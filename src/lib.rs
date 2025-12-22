#![allow(private_interfaces)]

use sha3::{Digest, Keccak256};
use std::{
    ffi::CStr,
    os::raw::c_char,
    path::PathBuf,
    sync::Arc,
};

use once_cell::sync::OnceCell;
use rand::{rngs::StdRng, RngCore, SeedableRng};
use tokio::runtime::Runtime;

use miden_client::{
    account::component::BasicWallet,
    auth::AuthSecretKey,
    builder::ClientBuilder,
    keystore::FilesystemKeyStore,
    rpc::{Endpoint, GrpcClient},
    transaction::TransactionRequestBuilder,
    Client,
};
use miden_client_sqlite_store::ClientBuilderSqliteExt;
use miden_lib::account::auth::AuthRpoFalcon512;
use miden_objects::account::{
    Account, AccountBuilder, AccountComponent, AccountId, AccountStorageMode, AccountType,
};
use miden_objects::note::NoteId;

// ================================================================================================
// Global State
// ================================================================================================

/// Global Tokio Runtime
static RUNTIME: OnceCell<Runtime> = OnceCell::new();

/// Miden Client type aliases
type MidenKeyStore = FilesystemKeyStore<StdRng>;
type MidenClient = Client<MidenKeyStore>;

/// Get or create Runtime
fn get_runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to create Tokio runtime")
    })
}

/// Execute async code in Runtime context
/// 
/// Uses Runtime::block_on to ensure execution in the correct Tokio context
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    get_runtime().block_on(future)
}

// ================================================================================================
// Client Handle (Handle-based API for FFI)
// ================================================================================================

/// Client context containing all required resources
struct MidenContext {
    client: MidenClient,
    keystore: Arc<MidenKeyStore>,
}

/// Opaque handle type
pub type MidenHandle = *mut MidenContext;

// ================================================================================================
// Miden Client FFI Interface
// ================================================================================================

/// Create and initialize Miden Client
/// 
/// # Parameters
/// - `keystore_path`: Keystore storage directory path (C string)
/// - `store_path`: SQLite database file path (C string)
/// - `rpc_endpoint`: RPC endpoint URL (C string, can be NULL to use testnet)
/// - `handle_out`: Output client handle
/// 
/// # Returns
/// - 0: Success
/// - -1: Invalid parameters
/// - -2: Initialization failed
/// 
/// # Note
/// The caller is responsible for calling `wc_miden_destroy` to release resources after use
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_create(
    keystore_path: *const c_char,
    store_path: *const c_char,
    rpc_endpoint: *const c_char,
    handle_out: *mut MidenHandle,
) -> i32 {
    // Parameter validation
    if keystore_path.is_null() || store_path.is_null() || handle_out.is_null() {
        return -1;
    }

    // Parse paths
    let keystore_path = match unsafe { CStr::from_ptr(keystore_path) }.to_str() {
        Ok(s) => PathBuf::from(s),
        Err(_) => return -1,
    };
    
    let store_path = match unsafe { CStr::from_ptr(store_path) }.to_str() {
        Ok(s) => PathBuf::from(s),
        Err(_) => return -1,
    };

    // Parse RPC endpoint (optional)
    let endpoint = if rpc_endpoint.is_null() {
        Endpoint::testnet()
    } else {
        match unsafe { CStr::from_ptr(rpc_endpoint) }.to_str() {
            Ok(s) => {
                if s.is_empty() || s == "testnet" {
                    Endpoint::testnet()
                } else {
                    // Custom endpoint parsing logic can be added here
                    Endpoint::testnet()
                }
            }
            Err(_) => Endpoint::testnet(),
        }
    };

    // Initialize (execute in Runtime context)
    let result = block_on(async {
        create_context_async(keystore_path, store_path, endpoint).await
    });

    match result {
        Ok(context) => {
            let boxed = Box::new(context);
            unsafe { *handle_out = Box::into_raw(boxed) };
            0
        }
        Err(_) => -2,
    }
}

/// Asynchronously create Context
async fn create_context_async(
    keystore_path: PathBuf,
    store_path: PathBuf,
    endpoint: Endpoint,
) -> Result<MidenContext, Box<dyn std::error::Error + Send + Sync>> {
    // Create directories if they don't exist
    if let Some(parent) = keystore_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::create_dir_all(&keystore_path).ok();

    // Initialize keystore
    let keystore = Arc::new(
        FilesystemKeyStore::<StdRng>::new(keystore_path)
            .map_err(|e| format!("Failed to create keystore: {:?}", e))?
    );

    // Create RPC client
    let timeout_ms = 10_000;
    let rpc_client = Arc::new(GrpcClient::new(&endpoint, timeout_ms));

    // Build Client
    let client = ClientBuilder::new()
        .rpc(rpc_client)
        .sqlite_store(store_path)
        .authenticator(keystore.clone())
        .in_debug_mode(false.into())
        .build()
        .await
        .map_err(|e| format!("Failed to build client: {:?}", e))?;

    Ok(MidenContext { client, keystore })
}

/// Destroy client and release resources
/// 
/// # Parameters
/// - `handle`: Client handle
/// 
/// # Note
/// Must execute drop in Tokio runtime context, because SQLite connection pool's
/// SyncWrapper::drop needs to call spawn_blocking_background
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_destroy(handle: MidenHandle) {
    if !handle.is_null() {
        // Execute drop in runtime context
        // This is required for deadpool-sync's SyncWrapper::drop
        block_on(async {
            unsafe {
                let _ = Box::from_raw(handle);
            }
        });
    }
}

/// Sync state
/// 
/// # Parameters
/// - `handle`: Client handle
/// - `block_num_out`: Output latest block number (can be NULL)
/// 
/// # Returns
/// - 0: Success
/// - -1: Invalid handle
/// - -2: Sync failed
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_sync(handle: MidenHandle, block_num_out: *mut u32) -> i32 {
    if handle.is_null() {
        return -1;
    }

    let context = unsafe { &mut *handle };
    
    let result = block_on(async {
        context.client.sync_state().await
    });

    match result {
        Ok(summary) => {
            if !block_num_out.is_null() {
                unsafe { *block_num_out = summary.block_num.as_u32() };
            }
            0
        }
        Err(_) => -2,
    }
}

/// Create a new Miden wallet account
/// 
/// # Parameters
/// - `handle`: Client handle
/// - `seed_ptr`: 32-byte random seed (if NULL, auto-generated)
/// - `seed_len`: Seed length (must be 32, ignored if seed_ptr is NULL)
/// - `account_id_out`: Output buffer for account ID (at least 64 bytes for hex string)
/// - `account_id_out_len`: Input as buffer size, output as actual length
/// 
/// # Returns
/// - 0: Success
/// - -1: Invalid parameters
/// - -2: Invalid handle
/// - -3: Account creation failed
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_create_wallet(
    handle: MidenHandle,
    seed_ptr: *const u8,
    seed_len: usize,
    account_id_out: *mut u8,
    account_id_out_len: *mut usize,
) -> i32 {
    // Parameter validation
    if handle.is_null() {
        return -2;
    }
    if account_id_out.is_null() || account_id_out_len.is_null() {
        return -1;
    }

    // Get or generate seed
    let init_seed: [u8; 32] = if seed_ptr.is_null() {
        // Auto-generate seed
        let mut seed = [0u8; 32];
        let mut rng = StdRng::from_os_rng();
        rng.fill_bytes(&mut seed);
        seed
    } else {
        if seed_len != 32 {
            return -1;
        }
        let seed = unsafe { std::slice::from_raw_parts(seed_ptr, seed_len) };
        let mut arr = [0u8; 32];
        arr.copy_from_slice(seed);
        arr
    };

    let context = unsafe { &mut *handle };
    
    let result = block_on(async {
        create_wallet_async(&mut context.client, &context.keystore, init_seed).await
    });

    match result {
        Ok(account) => {
            // Output account ID (hex string)
            let account_id_hex = account.id().to_hex();
            let out_capacity = unsafe { *account_id_out_len };
            
            if account_id_hex.len() > out_capacity {
                return -1;
            }

            let out = unsafe { std::slice::from_raw_parts_mut(account_id_out, account_id_hex.len()) };
            out.copy_from_slice(account_id_hex.as_bytes());
            unsafe { *account_id_out_len = account_id_hex.len() };

            0
        }
        Err(_) => -3,
    }
}

/// Asynchronously create wallet
async fn create_wallet_async(
    client: &mut MidenClient,
    keystore: &Arc<MidenKeyStore>,
    init_seed: [u8; 32],
) -> Result<Account, Box<dyn std::error::Error + Send + Sync>> {
    // Create key pair (using RPO Falcon512 authentication scheme)
    let key_pair = AuthSecretKey::new_rpo_falcon512();
    let auth_component: AccountComponent =
        AuthRpoFalcon512::new(key_pair.public_key().to_commitment()).into();

    // Save key to keystore
    keystore.add_key(&key_pair)
        .map_err(|e| format!("Failed to add key: {:?}", e))?;

    // Build account
    let account = AccountBuilder::new(init_seed)
        .account_type(AccountType::RegularAccountImmutableCode)
        .storage_mode(AccountStorageMode::Public)
        .with_auth_component(auth_component)
        .with_component(BasicWallet)
        .build()
        .map_err(|e| format!("Failed to build account: {:?}", e))?;

    // Add account to client
    client.add_account(&account, false).await
        .map_err(|e| format!("Failed to add account: {:?}", e))?;
    // client.deploy_account(&account).await;
    Ok(account)
}

/// Get all accounts list
/// 
/// # Parameters
/// - `handle`: Client handle
/// - `accounts_json_out`: Output buffer for JSON-formatted account list
/// - `accounts_json_out_len`: Input as buffer size, output as actual length
/// 
/// # Returns
/// - 0: Success
/// - -1: Invalid parameters
/// - -2: Invalid handle
/// - -3: Get failed
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_get_accounts(
    handle: MidenHandle,
    accounts_json_out: *mut u8,
    accounts_json_out_len: *mut usize,
) -> i32 {
    if handle.is_null() {
        return -2;
    }
    if accounts_json_out.is_null() || accounts_json_out_len.is_null() {
        return -1;
    }

    let context = unsafe { &*handle };
    
    let result = block_on(async {
        context.client.get_account_headers().await
    });

    match result {
        Ok(accounts) => {
            // Build simple JSON array
            let account_ids: Vec<String> = accounts
                .iter()
                .map(|(header, _status)| header.id().to_hex())
                .collect();
            
            let json = format!("[{}]", 
                account_ids.iter()
                    .map(|id| format!("\"{}\"", id))
                    .collect::<Vec<_>>()
                    .join(",")
            );

            let out_capacity = unsafe { *accounts_json_out_len };
            if json.len() > out_capacity {
                return -1;
            }

            let out = unsafe { std::slice::from_raw_parts_mut(accounts_json_out, json.len()) };
            out.copy_from_slice(json.as_bytes());
            unsafe { *accounts_json_out_len = json.len() };

            0
        }
        Err(_) => -3,
    }
}

/// Get account balance
/// 
/// Returns JSON-formatted information about all assets in the account, including fungible and non-fungible assets.
/// 
/// # Parameters
/// - `handle`: Client handle
/// - `account_id_hex`: Account ID (hex string, e.g., "0x...")
/// - `balance_json_out`: Output buffer for JSON-formatted balance information
/// - `balance_json_out_len`: Input as buffer size, output as actual length
/// 
/// # Returns
/// - 0: Success
/// - -1: Invalid parameters
/// - -2: Invalid handle
/// - -3: Account ID parsing failed
/// - -4: Account not found
/// - -5: Get balance failed
/// 
/// # JSON 输出格式
/// ```json
/// {
///   "account_id": "0x...",
///   "fungible_assets": [
///     {"faucet_id": "0x...", "amount": 1000}
///   ],
///   "total_fungible_count": 1,
///   "total_non_fungible_count": 0
/// }
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_get_balance(
    handle: MidenHandle,
    account_id_hex: *const c_char,
    balance_json_out: *mut u8,
    balance_json_out_len: *mut usize,
) -> i32 {
    // Parameter validation
    if handle.is_null() {
        return -2;
    }
    if account_id_hex.is_null() || balance_json_out.is_null() || balance_json_out_len.is_null() {
        return -1;
    }

    // Parse account ID
    let account_id_str = match unsafe { CStr::from_ptr(account_id_hex) }.to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };

    let account_id = match AccountId::from_hex(account_id_str) {
        Ok(id) => id,
        Err(_) => return -3,
    };

    let context = unsafe { &*handle };

    // Get account information
    let result = block_on(async {
        context.client.get_account(account_id).await
    });

    match result {
        Ok(Some(account_record)) => {
            let account = account_record.account();
            let vault = account.vault();

            // Collect fungible assets
            let mut fungible_assets = Vec::new();
            let mut non_fungible_count = 0u32;

            for asset in vault.assets() {
                if asset.is_fungible() {
                    let fungible = asset.unwrap_fungible();
                    fungible_assets.push(format!(
                        r#"{{"faucet_id":"{}","amount":{}}}"#,
                        fungible.faucet_id().to_hex(),
                        fungible.amount()
                    ));
                } else {
                    non_fungible_count += 1;
                }
            }

            // Build JSON
            let json = format!(
                r#"{{"account_id":"{}","fungible_assets":[{}],"total_fungible_count":{},"total_non_fungible_count":{}}}"#,
                account_id_str,
                fungible_assets.join(","),
                fungible_assets.len(),
                non_fungible_count
            );

            // Output
            let out_capacity = unsafe { *balance_json_out_len };
            if json.len() > out_capacity {
                return -1;
            }

            let out = unsafe { std::slice::from_raw_parts_mut(balance_json_out, json.len()) };
            out.copy_from_slice(json.as_bytes());
            unsafe { *balance_json_out_len = json.len() };

            0
        }
        Ok(None) => -4, // Account not found
        Err(_) => -5,   // Get failed
    }
}

/// Test Miden Client connection
/// 
/// # Parameters
/// - `handle`: Client handle
/// 
/// # Returns
/// - 0: Connection OK
/// - -1: Invalid handle
/// - -2: Connection failed
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_test_connection(handle: MidenHandle) -> i32 {
    if handle.is_null() {
        return -1;
    }

    let context = unsafe { &mut *handle };
    
    let result = block_on(async {
        context.client.sync_state().await
    });

    match result {
        Ok(_) => 0,
        Err(_) => -2,
    }
}

/// Get consumable Input Notes
/// 
/// Returns all consumable notes (unspent, committed notes).
/// 
/// # Parameters
/// - `handle`: Client handle
/// - `account_id_hex`: Account ID (hex string, can be NULL to get notes for all accounts)
/// - `notes_json_out`: Output buffer for JSON-formatted notes list
/// - `notes_json_out_len`: Input as buffer size, output as actual length
/// 
/// # Returns
/// - 0: Success
/// - -1: Invalid parameters
/// - -2: Invalid handle
/// - -3: Account ID parsing failed
/// - -4: Get failed
/// 
/// # JSON 输出格式
/// ```json
/// {
///   "notes": [
///     {
///       "note_id": "0x...",
///       "assets": [{"faucet_id": "0x...", "amount": 1000}],
///       "is_authenticated": true
///     }
///   ],
///   "total_count": 1
/// }
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_get_input_notes(
    handle: MidenHandle,
    account_id_hex: *const c_char,
    notes_json_out: *mut u8,
    notes_json_out_len: *mut usize,
) -> i32 {
    // Parameter validation
    if handle.is_null() {
        return -2;
    }
    if notes_json_out.is_null() || notes_json_out_len.is_null() {
        return -1;
    }

    // Parse account ID (optional)
    let account_id: Option<AccountId> = if account_id_hex.is_null() {
        None
    } else {
        match unsafe { CStr::from_ptr(account_id_hex) }.to_str() {
            Ok(s) if s.is_empty() => None,
            Ok(s) => match AccountId::from_hex(s) {
                Ok(id) => Some(id),
                Err(_) => return -3,
            },
            Err(_) => return -1,
        }
    };

    let context = unsafe { &*handle };

    // Get consumable notes
    let result = block_on(async {
        context.client.get_consumable_notes(account_id).await
    });

    match result {
        Ok(consumable_notes) => {
            // Build JSON
            let notes_json: Vec<String> = consumable_notes
                .iter()
                .map(|(note_record, _consumability)| {
                    // Collect assets
                    let assets_json: Vec<String> = note_record
                        .assets()
                        .iter()
                        .filter_map(|asset| {
                            if asset.is_fungible() {
                                let fungible = asset.unwrap_fungible();
                                Some(format!(
                                    r#"{{"faucet_id":"{}","amount":{}}}"#,
                                    fungible.faucet_id().to_hex(),
                                    fungible.amount()
                                ))
                            } else {
                                None
                            }
                        })
                        .collect();

                    format!(
                        r#"{{"note_id":"{}","assets":[{}],"is_authenticated":{}}}"#,
                        note_record.id().to_hex(),
                        assets_json.join(","),
                        note_record.is_authenticated()
                    )
                })
                .collect();

            let json = format!(
                r#"{{"notes":[{}],"total_count":{}}}"#,
                notes_json.join(","),
                consumable_notes.len()
            );

            // Output
            let out_capacity = unsafe { *notes_json_out_len };
            if json.len() > out_capacity {
                return -1;
            }

            let out = unsafe { std::slice::from_raw_parts_mut(notes_json_out, json.len()) };
            out.copy_from_slice(json.as_bytes());
            unsafe { *notes_json_out_len = json.len() };

            0
        }
        Err(_) => -4,
    }
}

/// Consume Notes
/// 
/// Create and submit a transaction to consume specified notes.
/// 
/// # Parameters
/// - `handle`: Client handle
/// - `account_id_hex`: Account ID to execute transaction (hex string)
/// - `note_ids_json`: JSON-formatted array of note IDs (e.g., `["0x...", "0x..."]`)
/// - `tx_id_out`: Output buffer for transaction ID (at least 64 bytes)
/// - `tx_id_out_len`: Input as buffer size, output as actual length
/// 
/// # Returns
/// - 0: Success
/// - -1: Invalid parameters
/// - -2: Invalid handle
/// - -3: Account ID parsing failed
/// - -4: Note IDs parsing failed
/// - -5: Transaction creation failed
/// - -6: Transaction submission failed
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_consume_notes(
    handle: MidenHandle,
    account_id_hex: *const c_char,
    note_ids_json: *const c_char,
    tx_id_out: *mut u8,
    tx_id_out_len: *mut usize,
) -> i32 {
    // Parameter validation
    if handle.is_null() {
        return -2;
    }
    if account_id_hex.is_null() || note_ids_json.is_null() {
        return -1;
    }
    if tx_id_out.is_null() || tx_id_out_len.is_null() {
        return -1;
    }

    // Parse account ID
    let account_id_str = match unsafe { CStr::from_ptr(account_id_hex) }.to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let account_id = match AccountId::from_hex(account_id_str) {
        Ok(id) => id,
        Err(_) => return -3,
    };

    // Parse note IDs JSON
    let note_ids_str = match unsafe { CStr::from_ptr(note_ids_json) }.to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };

    // Simple JSON array parsing ["0x...", "0x..."]
    let note_ids: Vec<NoteId> = match parse_note_ids_json(note_ids_str) {
        Ok(ids) => ids,
        Err(_) => return -4,
    };

    if note_ids.is_empty() {
        return -4;
    }

    let context = unsafe { &mut *handle };

    // Build and submit transaction
    let result = block_on(async {
        consume_notes_async(&mut context.client, account_id, note_ids).await
    });

    match result {
        Ok(tx_id_hex) => {
            let out_capacity = unsafe { *tx_id_out_len };
            if tx_id_hex.len() > out_capacity {
                return -1;
            }

            let out = unsafe { std::slice::from_raw_parts_mut(tx_id_out, tx_id_hex.len()) };
            out.copy_from_slice(tx_id_hex.as_bytes());
            unsafe { *tx_id_out_len = tx_id_hex.len() };

            0
        }
        Err(e) => {
            // Return different error codes based on error type
            if e.contains("request") || e.contains("build") {
                -5
            } else {
                -6
            }
        }
    }
}

/// Parse note IDs JSON array
fn parse_note_ids_json(json: &str) -> Result<Vec<NoteId>, String> {
    let trimmed = json.trim();
    if !trimmed.starts_with('[') || !trimmed.ends_with(']') {
        return Err("Invalid JSON array".to_string());
    }

    let inner = &trimmed[1..trimmed.len() - 1];
    if inner.trim().is_empty() {
        return Ok(Vec::new());
    }

    let mut note_ids = Vec::new();
    for part in inner.split(',') {
        let part = part.trim();
        // Remove quotes
        let id_str = part.trim_matches('"').trim_matches('\'');
        let note_id = NoteId::try_from_hex(id_str)
            .map_err(|e| format!("Invalid note ID {}: {:?}", id_str, e))?;
        note_ids.push(note_id);
    }

    Ok(note_ids)
}

/// Asynchronously consume notes
async fn consume_notes_async(
    client: &mut MidenClient,
    account_id: AccountId,
    note_ids: Vec<NoteId>,
) -> Result<String, String> {
    // Build consume transaction request
    let tx_request = TransactionRequestBuilder::new()
        .build_consume_notes(note_ids)
        .map_err(|e| format!("Failed to build transaction request: {:?}", e))?;

    // Submit transaction
    let tx_id = client
        .submit_new_transaction(account_id, tx_request)
        .await
        .map_err(|e| format!("Failed to submit transaction: {:?}", e))?;

    Ok(tx_id.to_hex())
}

// ================================================================================================
// Keccak256 Hash Function
// ================================================================================================

pub fn keccak256_bytes(data: &[u8]) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    hasher.update(data);
    let out = hasher.finalize();
    
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&out);
    arr
}

pub fn keccak256_bytes_v2(data: &[u8]) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    hasher.update(data);
    let out = hasher.finalize();
    out.into()
}

pub fn keccak256_bytes_v3(data: &[u8]) -> [u8; 32] {
    Keccak256::digest(data).into()
}

#[unsafe(no_mangle)]
pub extern "C" fn wc_keccak256(
    data_ptr: *const u8,
    data_len: usize,
    out_ptr: *mut u8,
    out_len: *mut usize,
) -> i32 {
    // Safety boundary check
    if data_ptr.is_null() || out_ptr.is_null() || out_len.is_null() {
        return -1;
    }
    let data = unsafe { std::slice::from_raw_parts(data_ptr, data_len) };

    // keccak256
    let mut hasher = Keccak256::new();
    hasher.update(data);
    let result = hasher.finalize(); // 32 bytes

    // Copy to caller's buffer
    let out = unsafe { std::slice::from_raw_parts_mut(out_ptr, 32) };
    out.copy_from_slice(&result[..]);
    unsafe { *out_len = 32 };
    0
}

/// Convert account ID to hex string
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_account_id_to_hex(
    account_id_ptr: *const u8,
    account_id_len: usize,
    hex_out: *mut u8,
    hex_out_len: *mut usize,
) -> i32 {
    if account_id_ptr.is_null() || hex_out.is_null() || hex_out_len.is_null() {
        return -1;
    }

    let account_id_bytes = unsafe { std::slice::from_raw_parts(account_id_ptr, account_id_len) };
    let hex_string = hex::encode(account_id_bytes);
    
    let out_capacity = unsafe { *hex_out_len };
    if hex_string.len() > out_capacity {
        return -1;
    }

    let out = unsafe { std::slice::from_raw_parts_mut(hex_out, hex_string.len()) };
    out.copy_from_slice(hex_string.as_bytes());
    unsafe { *hex_out_len = hex_string.len() };

    0
}
