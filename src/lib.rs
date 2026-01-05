#![allow(private_interfaces)]

use sha3::{Digest, Keccak256};
use std::{
    ffi::CStr,
    os::raw::c_char,
    path::PathBuf,
    sync::Arc,
    thread::JoinHandle,
    time::Duration,
};

/// Default timeout for synchronous operations (30 seconds)
pub const SYNC_TIMEOUT: Duration = Duration::from_secs(30);

/// Maximum number of pending requests in the worker queue
/// Prevents unbounded memory growth if caller spams requests
pub const WORKER_QUEUE_CAPACITY: usize = 256;

// ================================================================================================
// Global Error Codes
// ================================================================================================
//
// Standard error codes used across all FFI functions:
//
//   0:    Success
//  -1:    Invalid parameter (null pointer, invalid format, buffer too small)
//  -2:    Invalid handle or worker closed
//  -3:    Account/key operation failed
//  -4:    Note operation failed / invalid note ID
//  -5:    Balance/account lookup failed
//  -6:    Transaction submission failed
//  -8:    Queue full (too many pending requests)
//  -99:   Operation timed out (sync API only)
//
// Business-specific errors use -100 to -199 range (reserved for future use)
//

/// Error: invalid parameter
pub const ERR_INVALID_PARAM: i32 = -1;
/// Error: invalid handle or worker closed  
pub const ERR_INVALID_HANDLE: i32 = -2;
/// Error: account/key operation failed
pub const ERR_ACCOUNT_OP: i32 = -3;
/// Error: note operation failed
pub const ERR_NOTE_OP: i32 = -4;
/// Error: balance/account lookup failed
pub const ERR_LOOKUP: i32 = -5;
/// Error: transaction submission failed
pub const ERR_TX_SUBMIT: i32 = -6;
/// Error: worker queue is full
pub const ERR_QUEUE_FULL: i32 = -8;
/// Error: operation timed out
pub const ERR_TIMEOUT: i32 = -99;

use rand::{rngs::StdRng, RngCore, SeedableRng};
use tokio::sync::mpsc;

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
    AccountBuilder, AccountComponent, AccountId, AccountStorageMode, AccountType,
};
use miden_objects::note::NoteId;

// ================================================================================================
// Type Aliases
// ================================================================================================

type MidenKeyStore = FilesystemKeyStore<StdRng>;
type MidenClient = Client<MidenKeyStore>;

// ================================================================================================
// Async Callback Types
// ================================================================================================

/// Callback for sync operation: (user_data, error_code, block_num)
pub type SyncCallback = extern "C" fn(*mut std::ffi::c_void, i32, u32);

/// Callback for create wallet operation: (user_data, error_code, account_id_ptr, account_id_len)
pub type CreateWalletCallback = extern "C" fn(*mut std::ffi::c_void, i32, *mut u8, usize);

/// Callback for get accounts operation: (user_data, error_code, json_ptr, json_len)
pub type GetAccountsCallback = extern "C" fn(*mut std::ffi::c_void, i32, *mut u8, usize);

/// Callback for get balance operation: (user_data, error_code, json_ptr, json_len)
pub type GetBalanceCallback = extern "C" fn(*mut std::ffi::c_void, i32, *mut u8, usize);

/// Callback for get input notes operation: (user_data, error_code, json_ptr, json_len)
pub type GetInputNotesCallback = extern "C" fn(*mut std::ffi::c_void, i32, *mut u8, usize);

/// Callback for consume notes operation: (user_data, error_code, tx_id_ptr, tx_id_len)
pub type ConsumeNotesCallback = extern "C" fn(*mut std::ffi::c_void, i32, *mut u8, usize);

/// Callback for test connection operation: (user_data, error_code)
pub type TestConnectionCallback = extern "C" fn(*mut std::ffi::c_void, i32);

// ================================================================================================
// Worker Thread Architecture
// ================================================================================================

/// Request types sent to the worker thread
enum Request {
    // Sync operations (blocking, return result via oneshot channel)
    SyncSync {
        response_tx: std::sync::mpsc::Sender<SyncResult>,
    },
    CreateWalletSync {
        seed: [u8; 32],
        response_tx: std::sync::mpsc::Sender<CreateWalletResult>,
    },
    GetAccountsSync {
        response_tx: std::sync::mpsc::Sender<GetAccountsResult>,
    },
    GetBalanceSync {
        account_id: AccountId,
        account_id_str: String,
        response_tx: std::sync::mpsc::Sender<GetBalanceResult>,
    },
    GetInputNotesSync {
        account_id: Option<AccountId>,
        response_tx: std::sync::mpsc::Sender<GetInputNotesResult>,
    },
    ConsumeNotesSync {
        account_id: AccountId,
        note_ids: Vec<NoteId>,
        response_tx: std::sync::mpsc::Sender<ConsumeNotesResult>,
    },
    TestConnectionSync {
        response_tx: std::sync::mpsc::Sender<TestConnectionResult>,
    },
    
    // Async operations (non-blocking, call callback when done)
    SyncAsync {
        callback: SyncCallback,
        user_data: usize,
    },
    CreateWalletAsync {
        seed: [u8; 32],
        callback: CreateWalletCallback,
        user_data: usize,
    },
    GetAccountsAsync {
        callback: GetAccountsCallback,
        user_data: usize,
    },
    GetBalanceAsync {
        account_id: AccountId,
        account_id_str: String,
        callback: GetBalanceCallback,
        user_data: usize,
    },
    GetInputNotesAsync {
        account_id: Option<AccountId>,
        callback: GetInputNotesCallback,
        user_data: usize,
    },
    ConsumeNotesAsync {
        account_id: AccountId,
        note_ids: Vec<NoteId>,
        callback: ConsumeNotesCallback,
        user_data: usize,
    },
    TestConnectionAsync {
        callback: TestConnectionCallback,
        user_data: usize,
    },
    
    // Control
    Shutdown,
}

// Result types for sync operations
type SyncResult = Result<u32, i32>;
type CreateWalletResult = Result<String, i32>;
type GetAccountsResult = Result<String, i32>;
type GetBalanceResult = Result<String, i32>;
type GetInputNotesResult = Result<String, i32>;
type ConsumeNotesResult = Result<String, i32>;
type TestConnectionResult = Result<(), i32>;

/// Client context (lives entirely in worker thread)
struct MidenContext {
    client: MidenClient,
    keystore: Arc<MidenKeyStore>,
}

/// Handle structure containing sender to worker thread
pub struct MidenWorkerHandle {
    /// Sender to worker thread (Option to allow taking/dropping in destroy)
    sender: Option<mpsc::Sender<Request>>,
    #[allow(dead_code)]
    worker_thread: Option<JoinHandle<()>>,
}

/// Opaque handle type for FFI
pub type MidenHandle = *mut MidenWorkerHandle;

// ================================================================================================
// Memory Management for FFI
// ================================================================================================

/// Free bytes allocated by Rust (for async callback results)
/// 
/// MUST be called to release memory returned by async callbacks.
/// The (ptr, len) pair must match exactly what was returned by the callback.
#[unsafe(no_mangle)]
pub extern "C" fn wc_bytes_free(ptr: *mut u8, len: usize) {
    if ptr.is_null() {
        return;
    }
    // Reconstruct Vec from raw parts and drop it
    // This matches the allocation in leak_bytes()
    unsafe { drop(Vec::from_raw_parts(ptr, len, len)); }
}

/// Leak a Vec<u8> for FFI, returning (ptr, len)
/// 
/// The caller is responsible for calling wc_bytes_free(ptr, len) to release.
fn leak_bytes(mut v: Vec<u8>) -> (*mut u8, usize) {
    let ptr = v.as_mut_ptr();
    let len = v.len();
    std::mem::forget(v);
    (ptr, len)
}

// ================================================================================================
// Worker Thread Implementation
// ================================================================================================

/// Start worker thread with single-threaded Tokio runtime
fn start_worker(
    keystore_path: PathBuf,
    store_path: PathBuf,
    endpoint: Endpoint,
) -> Result<MidenWorkerHandle, String> {
    let (tx, rx) = mpsc::channel::<Request>(WORKER_QUEUE_CAPACITY);
    
    // Use std channel for init result
    let (init_tx, init_rx) = std::sync::mpsc::channel::<Result<(), String>>();
    
    let worker_thread = std::thread::spawn(move || {
        // Create single-threaded Tokio runtime
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create Tokio runtime");
        
        rt.block_on(async move {
            // Initialize context
            let context = match create_context_async(keystore_path, store_path, endpoint).await {
                Ok(ctx) => {
                    let _ = init_tx.send(Ok(()));
                    ctx
                }
                Err(e) => {
                    let _ = init_tx.send(Err(e));
                    return;
                }
            };
            
            // Run event loop
            worker_event_loop(context, rx).await;
        });
    });
    
    // Wait for initialization result
    match init_rx.recv() {
        Ok(Ok(())) => Ok(MidenWorkerHandle {
            sender: Some(tx),
            worker_thread: Some(worker_thread),
        }),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("Worker thread initialization failed".to_string()),
    }
}

/// Asynchronously create MidenContext
async fn create_context_async(
    keystore_path: PathBuf,
    store_path: PathBuf,
    endpoint: Endpoint,
) -> Result<MidenContext, String> {
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

/// Worker event loop - processes requests sequentially
async fn worker_event_loop(mut context: MidenContext, mut rx: mpsc::Receiver<Request>) {
    while let Some(request) = rx.recv().await {
        match request {
            Request::Shutdown => break,
            
            // Sync operations
            Request::SyncSync { response_tx } => {
                let result = context.client.sync_state().await;
                let _ = response_tx.send(match result {
                    Ok(summary) => Ok(summary.block_num.as_u32()),
                    Err(e) => {
                        eprintln!("[wc_miden_sync] sync_state failed: {:?}", e);
                        Err(ERR_INVALID_HANDLE)
                    }
                });
            }
            
            Request::CreateWalletSync { seed, response_tx } => {
                let result = create_wallet_impl(&mut context, seed).await;
                let _ = response_tx.send(result);
            }
            
            Request::GetAccountsSync { response_tx } => {
                let result = get_accounts_impl(&context).await;
                let _ = response_tx.send(result);
            }
            
            Request::GetBalanceSync { account_id, account_id_str, response_tx } => {
                let result = get_balance_impl(&context, account_id, &account_id_str).await;
                let _ = response_tx.send(result);
            }
            
            Request::GetInputNotesSync { account_id, response_tx } => {
                let result = get_input_notes_impl(&context, account_id).await;
                let _ = response_tx.send(result);
            }
            
            Request::ConsumeNotesSync { account_id, note_ids, response_tx } => {
                let result = consume_notes_impl(&mut context, account_id, note_ids).await;
                let _ = response_tx.send(result);
            }
            
            Request::TestConnectionSync { response_tx } => {
                let result = context.client.sync_state().await;
                let _ = response_tx.send(match result {
                    Ok(_) => Ok(()),
                    Err(_) => Err(ERR_INVALID_HANDLE),
                });
            }
            
            // Async operations
            Request::SyncAsync { callback, user_data } => {
                let result = context.client.sync_state().await;
                let user_data_ptr = user_data as *mut std::ffi::c_void;
                match result {
                    Ok(summary) => callback(user_data_ptr, 0, summary.block_num.as_u32()),
                    Err(_) => callback(user_data_ptr, ERR_INVALID_HANDLE, 0),
                }
            }
            
            Request::CreateWalletAsync { seed, callback, user_data } => {
                let result = create_wallet_impl(&mut context, seed).await;
                let user_data_ptr = user_data as *mut std::ffi::c_void;
                match result {
                    Ok(account_id_hex) => {
                        let (ptr, len) = leak_bytes(account_id_hex.into_bytes());
                        callback(user_data_ptr, 0, ptr, len);
                    }
                    Err(code) => callback(user_data_ptr, code, std::ptr::null_mut(), 0),
                }
            }
            
            Request::GetAccountsAsync { callback, user_data } => {
                let result = get_accounts_impl(&context).await;
                let user_data_ptr = user_data as *mut std::ffi::c_void;
                match result {
                    Ok(json) => {
                        let (ptr, len) = leak_bytes(json.into_bytes());
                        callback(user_data_ptr, 0, ptr, len);
                    }
                    Err(code) => callback(user_data_ptr, code, std::ptr::null_mut(), 0),
                }
            }
            
            Request::GetBalanceAsync { account_id, account_id_str, callback, user_data } => {
                let result = get_balance_impl(&context, account_id, &account_id_str).await;
                let user_data_ptr = user_data as *mut std::ffi::c_void;
                match result {
                    Ok(json) => {
                        let (ptr, len) = leak_bytes(json.into_bytes());
                        callback(user_data_ptr, 0, ptr, len);
                    }
                    Err(code) => callback(user_data_ptr, code, std::ptr::null_mut(), 0),
                }
            }
            
            Request::GetInputNotesAsync { account_id, callback, user_data } => {
                let result = get_input_notes_impl(&context, account_id).await;
                let user_data_ptr = user_data as *mut std::ffi::c_void;
                match result {
                    Ok(json) => {
                        let (ptr, len) = leak_bytes(json.into_bytes());
                        callback(user_data_ptr, 0, ptr, len);
                    }
                    Err(code) => callback(user_data_ptr, code, std::ptr::null_mut(), 0),
                }
            }
            
            Request::ConsumeNotesAsync { account_id, note_ids, callback, user_data } => {
                let result = consume_notes_impl(&mut context, account_id, note_ids).await;
                let user_data_ptr = user_data as *mut std::ffi::c_void;
                match result {
                    Ok(tx_id_hex) => {
                        let (ptr, len) = leak_bytes(tx_id_hex.into_bytes());
                        callback(user_data_ptr, 0, ptr, len);
                    }
                    Err(code) => callback(user_data_ptr, code, std::ptr::null_mut(), 0),
                }
            }
            
            Request::TestConnectionAsync { callback, user_data } => {
                let result = context.client.sync_state().await;
                let user_data_ptr = user_data as *mut std::ffi::c_void;
                match result {
                    Ok(_) => callback(user_data_ptr, 0),
                    Err(_) => callback(user_data_ptr, ERR_INVALID_HANDLE),
                }
            }
        }
    }
}

// ================================================================================================
// Business Logic Implementations
// ================================================================================================

async fn create_wallet_impl(context: &mut MidenContext, init_seed: [u8; 32]) -> Result<String, i32> {
    // Create key pair
    let key_pair = AuthSecretKey::new_rpo_falcon512();
    let auth_component: AccountComponent =
        AuthRpoFalcon512::new(key_pair.public_key().to_commitment()).into();

    // Save key to keystore
    context.keystore.add_key(&key_pair)
        .map_err(|_| ERR_ACCOUNT_OP)?;

    // Build account
    let account = AccountBuilder::new(init_seed)
        .account_type(AccountType::RegularAccountImmutableCode)
        .storage_mode(AccountStorageMode::Public)
        .with_auth_component(auth_component)
        .with_component(BasicWallet)
        .build()
        .map_err(|_| ERR_ACCOUNT_OP)?;

    // Add account to client
    context.client.add_account(&account, false).await
        .map_err(|_| ERR_ACCOUNT_OP)?;

    Ok(account.id().to_hex())
}

async fn get_accounts_impl(context: &MidenContext) -> Result<String, i32> {
    let accounts = context.client.get_account_headers().await
        .map_err(|_| ERR_ACCOUNT_OP)?;
    
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
    
    Ok(json)
}

async fn get_balance_impl(context: &MidenContext, account_id: AccountId, account_id_str: &str) -> Result<String, i32> {
    let account_record = context.client.get_account(account_id).await
        .map_err(|_| ERR_LOOKUP)?
        .ok_or(ERR_LOOKUP)?;  // Account not found
    
    let account = account_record.account();
    let vault = account.vault();

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

    let json = format!(
        r#"{{"account_id":"{}","fungible_assets":[{}],"total_fungible_count":{},"total_non_fungible_count":{}}}"#,
        account_id_str,
        fungible_assets.join(","),
        fungible_assets.len(),
        non_fungible_count
    );

    Ok(json)
}

async fn get_input_notes_impl(context: &MidenContext, account_id: Option<AccountId>) -> Result<String, i32> {
    let consumable_notes = context.client.get_consumable_notes(account_id).await
        .map_err(|_| ERR_NOTE_OP)?;
    
    let notes_json: Vec<String> = consumable_notes
        .iter()
        .map(|(note_record, _consumability)| {
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

    Ok(json)
}

async fn consume_notes_impl(context: &mut MidenContext, account_id: AccountId, note_ids: Vec<NoteId>) -> Result<String, i32> {
    let tx_request = TransactionRequestBuilder::new()
        .build_consume_notes(note_ids)
        .map_err(|_| ERR_NOTE_OP)?;

    let tx_id = context.client
        .submit_new_transaction(account_id, tx_request)
        .await
        .map_err(|_| ERR_TX_SUBMIT)?;

    Ok(tx_id.to_hex())
}

// ================================================================================================
// FFI Helper Functions
// ================================================================================================

/// Get a reference to the worker handle from a raw pointer
/// 
/// # Safety
/// The returned reference is only valid for the duration of the current function call.
/// Do NOT store this reference or pass it across function boundaries.
fn get_handle<'a>(handle: MidenHandle) -> Option<&'a MidenWorkerHandle> {
    if handle.is_null() {
        return None;
    }
    Some(unsafe { &*handle })
}

/// Try to send a request to the worker queue
/// 
/// Returns:
/// - Ok(()) if sent successfully
/// - Err(ERR_QUEUE_FULL) if queue is full
/// - Err(ERR_INVALID_HANDLE) if channel is closed or sender is None
fn try_send_request(sender: &Option<mpsc::Sender<Request>>, request: Request) -> Result<(), i32> {
    let sender = sender.as_ref().ok_or(ERR_INVALID_HANDLE)?;
    match sender.try_send(request) {
        Ok(()) => Ok(()),
        Err(mpsc::error::TrySendError::Full(_)) => Err(ERR_QUEUE_FULL),
        Err(mpsc::error::TrySendError::Closed(_)) => Err(ERR_INVALID_HANDLE),
    }
}

fn parse_account_id(account_id_hex: *const c_char) -> Result<(AccountId, String), i32> {
    if account_id_hex.is_null() {
        return Err(ERR_INVALID_PARAM);
    }
    
    let account_id_str = unsafe { CStr::from_ptr(account_id_hex) }
        .to_str()
        .map_err(|_| ERR_INVALID_PARAM)?
        .to_string();
    
    let account_id = AccountId::from_hex(&account_id_str)
        .map_err(|_| ERR_ACCOUNT_OP)?;
    
    Ok((account_id, account_id_str))
}

fn parse_optional_account_id(account_id_hex: *const c_char) -> Result<Option<AccountId>, i32> {
    if account_id_hex.is_null() {
        return Ok(None);
    }
    
    let s = unsafe { CStr::from_ptr(account_id_hex) }
        .to_str()
        .map_err(|_| ERR_INVALID_PARAM)?;
    
    if s.is_empty() {
        return Ok(None);
    }
    
    AccountId::from_hex(s)
        .map(Some)
        .map_err(|_| ERR_ACCOUNT_OP)
}

/// Parse JSON array of note IDs
/// 
/// Accepts formats like:
/// - `["0x...", "0x..."]`
/// - `["0x...",\n  "0x..."]` (with whitespace/newlines)
fn parse_note_ids_json(json: &str) -> Result<Vec<NoteId>, i32> {
    // Use serde_json for robust parsing
    let ids: Vec<String> = serde_json::from_str(json)
        .map_err(|_| ERR_NOTE_OP)?;
    
    let mut note_ids = Vec::with_capacity(ids.len());
    for id_str in ids {
        let note_id = NoteId::try_from_hex(&id_str)
            .map_err(|_| ERR_NOTE_OP)?;
        note_ids.push(note_id);
    }
    
    Ok(note_ids)
}

// ================================================================================================
// FFI Interface - Create/Destroy
// ================================================================================================

/// Create and initialize Miden Client
/// 
/// This starts a dedicated worker thread that owns the MidenClient.
/// All operations are sent to this worker thread via channels.
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
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_create(
    keystore_path: *const c_char,
    store_path: *const c_char,
    rpc_endpoint: *const c_char,
    handle_out: *mut MidenHandle,
) -> i32 {
    if keystore_path.is_null() || store_path.is_null() || handle_out.is_null() {
        return ERR_INVALID_PARAM;
    }

    let keystore_path = match unsafe { CStr::from_ptr(keystore_path) }.to_str() {
        Ok(s) => PathBuf::from(s),
        Err(_) => return -1,
    };
    
    let store_path = match unsafe { CStr::from_ptr(store_path) }.to_str() {
        Ok(s) => PathBuf::from(s),
        Err(_) => return -1,
    };

    let endpoint = if rpc_endpoint.is_null() {
        Endpoint::testnet()
    } else {
        match unsafe { CStr::from_ptr(rpc_endpoint) }.to_str() {
            Ok(s) if s.is_empty() || s == "testnet" => Endpoint::testnet(),
            Ok(_) => Endpoint::testnet(), // TODO: support custom endpoints
            Err(_) => Endpoint::testnet(),
        }
    };

    match start_worker(keystore_path, store_path, endpoint) {
        Ok(handle) => {
            let boxed = Box::new(handle);
            unsafe { *handle_out = Box::into_raw(boxed) };
            0
        }
        Err(_) => -2,
    }
}

/// Destroy client and release resources
/// 
/// Sends shutdown signal to worker thread and waits for it to finish.
/// Safe to call multiple times - the handle pointer is set to NULL after destruction.
/// 
/// # Parameters
/// - `handle_ptr`: Pointer to the handle (will be set to NULL after destruction)
/// 
/// # Example (Swift)
/// ```swift
/// var handle: MidenHandle? = ...
/// wc_miden_destroy(&handle)  // handle is now nil
/// ```
/// 
/// # Shutdown Semantics
/// This performs a **fast shutdown**: pending requests in the queue will be dropped.
/// Callbacks for in-flight async operations may not be invoked.
/// If you need graceful shutdown (process all pending requests), do not use this pattern.
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_destroy(handle_ptr: *mut MidenHandle) {
    if handle_ptr.is_null() {
        return;
    }
    
    let handle = unsafe { *handle_ptr };
    if handle.is_null() {
        return;
    }
    
    // Set to null BEFORE cleanup to prevent concurrent access
    unsafe { *handle_ptr = std::ptr::null_mut() };
    
    let mut worker_handle = unsafe { Box::from_raw(handle) };
    
    // Best-effort shutdown signal (may fail if queue is full)
    if let Some(sender) = worker_handle.sender.take() {
        let _ = sender.try_send(Request::Shutdown);
        // Drop sender to close channel - guarantees worker will exit
        // even if queue was full and Shutdown wasn't received
        drop(sender);
    }
    
    // Wait for worker thread to finish
    if let Some(jh) = worker_handle.worker_thread.take() {
        let _ = jh.join();
    }
}

// ================================================================================================
// FFI Interface - Sync Operations (Blocking)
// ================================================================================================

/// Sync state (blocking)
/// 
/// WARNING: This is a blocking call. Do NOT call from the main/UI thread.
/// Use wc_miden_sync_async for non-blocking operation.
/// 
/// NOTE: Timeout (-99) only abandons waiting; the operation may still complete in background.
/// 
/// # Returns
/// - 0: Success
/// - -2: Invalid handle or worker closed
/// - -8: Queue full
/// - -99: Operation timed out
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_sync(handle: MidenHandle, block_num_out: *mut u32) -> i32 {
    let Some(worker) = get_handle(handle) else {
        return ERR_INVALID_HANDLE;
    };
    
    let (tx, rx) = std::sync::mpsc::channel();
    
    if let Err(code) = try_send_request(&worker.sender, Request::SyncSync { response_tx: tx }) {
        return code;
    }
    
    match rx.recv_timeout(SYNC_TIMEOUT) {
        Ok(Ok(block_num)) => {
            if !block_num_out.is_null() {
                unsafe { *block_num_out = block_num };
            }
            0
        }
        Ok(Err(code)) => code,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => ERR_TIMEOUT,
        Err(_) => ERR_INVALID_HANDLE,
    }
}

/// Create a new wallet account (blocking)
/// 
/// WARNING: This is a blocking call. Do NOT call from the main/UI thread.
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_create_wallet(
    handle: MidenHandle,
    seed_ptr: *const u8,
    seed_len: usize,
    account_id_out: *mut u8,
    account_id_out_len: *mut usize,
) -> i32 {
    let Some(worker) = get_handle(handle) else {
        return ERR_INVALID_HANDLE;
    };
    
    if account_id_out.is_null() || account_id_out_len.is_null() {
        return ERR_INVALID_PARAM;
    }

    let seed: [u8; 32] = if seed_ptr.is_null() {
        let mut s = [0u8; 32];
        let mut rng = StdRng::from_os_rng();
        rng.fill_bytes(&mut s);
        s
    } else {
        if seed_len != 32 {
            return ERR_INVALID_PARAM;
        }
        let slice = unsafe { std::slice::from_raw_parts(seed_ptr, 32) };
        let mut arr = [0u8; 32];
        arr.copy_from_slice(slice);
        arr
    };

    let (tx, rx) = std::sync::mpsc::channel();
    
    if let Err(code) = try_send_request(&worker.sender, Request::CreateWalletSync { seed, response_tx: tx }) {
        return code;
    }
    
    match rx.recv_timeout(SYNC_TIMEOUT) {
        Ok(Ok(account_id_hex)) => {
            let out_capacity = unsafe { *account_id_out_len };
            if account_id_hex.len() > out_capacity {
                return ERR_INVALID_PARAM;
            }
            let out = unsafe { std::slice::from_raw_parts_mut(account_id_out, account_id_hex.len()) };
            out.copy_from_slice(account_id_hex.as_bytes());
            unsafe { *account_id_out_len = account_id_hex.len() };
            0
        }
        Ok(Err(code)) => code,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => ERR_TIMEOUT,
        Err(_) => ERR_INVALID_HANDLE,
    }
}

/// Get all accounts (blocking)
/// 
/// WARNING: This is a blocking call. Do NOT call from the main/UI thread.
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_get_accounts(
    handle: MidenHandle,
    accounts_json_out: *mut u8,
    accounts_json_out_len: *mut usize,
) -> i32 {
    let Some(worker) = get_handle(handle) else {
        return ERR_INVALID_HANDLE;
    };
    
    if accounts_json_out.is_null() || accounts_json_out_len.is_null() {
        return ERR_INVALID_PARAM;
    }

    let (tx, rx) = std::sync::mpsc::channel();
    
    if let Err(code) = try_send_request(&worker.sender, Request::GetAccountsSync { response_tx: tx }) {
        return code;
    }
    
    match rx.recv_timeout(SYNC_TIMEOUT) {
        Ok(Ok(json)) => {
            let out_capacity = unsafe { *accounts_json_out_len };
            if json.len() > out_capacity {
                return ERR_INVALID_PARAM;
            }
            let out = unsafe { std::slice::from_raw_parts_mut(accounts_json_out, json.len()) };
            out.copy_from_slice(json.as_bytes());
            unsafe { *accounts_json_out_len = json.len() };
            0
        }
        Ok(Err(code)) => code,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => ERR_TIMEOUT,
        Err(_) => ERR_INVALID_HANDLE,
    }
}

/// Get account balance (blocking)
/// 
/// WARNING: This is a blocking call. Do NOT call from the main/UI thread.
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_get_balance(
    handle: MidenHandle,
    account_id_hex: *const c_char,
    balance_json_out: *mut u8,
    balance_json_out_len: *mut usize,
) -> i32 {
    let Some(worker) = get_handle(handle) else {
        return ERR_INVALID_HANDLE;
    };
    
    if balance_json_out.is_null() || balance_json_out_len.is_null() {
        return ERR_INVALID_PARAM;
    }

    let (account_id, account_id_str) = match parse_account_id(account_id_hex) {
        Ok(v) => v,
        Err(code) => return code,
    };

    let (tx, rx) = std::sync::mpsc::channel();
    
    if let Err(code) = try_send_request(&worker.sender, Request::GetBalanceSync { 
        account_id, 
        account_id_str, 
        response_tx: tx 
    }) {
        return code;
    }
    
    match rx.recv_timeout(SYNC_TIMEOUT) {
        Ok(Ok(json)) => {
            let out_capacity = unsafe { *balance_json_out_len };
            if json.len() > out_capacity {
                return ERR_INVALID_PARAM;
            }
            let out = unsafe { std::slice::from_raw_parts_mut(balance_json_out, json.len()) };
            out.copy_from_slice(json.as_bytes());
            unsafe { *balance_json_out_len = json.len() };
            0
        }
        Ok(Err(code)) => code,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => ERR_TIMEOUT,
        Err(_) => ERR_INVALID_HANDLE,
    }
}

/// Test connection (blocking)
/// 
/// WARNING: This is a blocking call. Do NOT call from the main/UI thread.
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_test_connection(handle: MidenHandle) -> i32 {
    let Some(worker) = get_handle(handle) else {
        return ERR_INVALID_HANDLE;
    };
    
    let (tx, rx) = std::sync::mpsc::channel();
    
    if let Err(code) = try_send_request(&worker.sender, Request::TestConnectionSync { response_tx: tx }) {
        return code;
    }
    
    match rx.recv_timeout(SYNC_TIMEOUT) {
        Ok(Ok(())) => 0,
        Ok(Err(code)) => code,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => ERR_TIMEOUT,
        Err(_) => ERR_INVALID_HANDLE,
    }
}

/// Get consumable input notes (blocking)
/// 
/// WARNING: This is a blocking call. Do NOT call from the main/UI thread.
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_get_input_notes(
    handle: MidenHandle,
    account_id_hex: *const c_char,
    notes_json_out: *mut u8,
    notes_json_out_len: *mut usize,
) -> i32 {
    let Some(worker) = get_handle(handle) else {
        return ERR_INVALID_HANDLE;
    };
    
    if notes_json_out.is_null() || notes_json_out_len.is_null() {
        return ERR_INVALID_PARAM;
    }

    let account_id = match parse_optional_account_id(account_id_hex) {
        Ok(v) => v,
        Err(code) => return code,
    };

    let (tx, rx) = std::sync::mpsc::channel();
    
    if let Err(code) = try_send_request(&worker.sender, Request::GetInputNotesSync { account_id, response_tx: tx }) {
        return code;
    }
    
    match rx.recv_timeout(SYNC_TIMEOUT) {
        Ok(Ok(json)) => {
            let out_capacity = unsafe { *notes_json_out_len };
            if json.len() > out_capacity {
                return ERR_INVALID_PARAM;
            }
            let out = unsafe { std::slice::from_raw_parts_mut(notes_json_out, json.len()) };
            out.copy_from_slice(json.as_bytes());
            unsafe { *notes_json_out_len = json.len() };
            0
        }
        Ok(Err(code)) => code,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => ERR_TIMEOUT,
        Err(_) => ERR_INVALID_HANDLE,
    }
}

/// Consume notes (blocking)
/// 
/// WARNING: This is a blocking call. Do NOT call from the main/UI thread.
/// NOTE: Timeout (-99) only abandons waiting; the transaction may still be submitted.
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_consume_notes(
    handle: MidenHandle,
    account_id_hex: *const c_char,
    note_ids_json: *const c_char,
    tx_id_out: *mut u8,
    tx_id_out_len: *mut usize,
) -> i32 {
    let Some(worker) = get_handle(handle) else {
        return ERR_INVALID_HANDLE;
    };
    
    if note_ids_json.is_null() || tx_id_out.is_null() || tx_id_out_len.is_null() {
        return ERR_INVALID_PARAM;
    }

    let (account_id, _) = match parse_account_id(account_id_hex) {
        Ok(v) => v,
        Err(code) => return code,
    };

    let note_ids_str = match unsafe { CStr::from_ptr(note_ids_json) }.to_str() {
        Ok(s) => s,
        Err(_) => return ERR_INVALID_PARAM,
    };

    let note_ids = match parse_note_ids_json(note_ids_str) {
        Ok(ids) if !ids.is_empty() => ids,
        _ => return ERR_NOTE_OP,
    };

    let (tx, rx) = std::sync::mpsc::channel();
    
    if let Err(code) = try_send_request(&worker.sender, Request::ConsumeNotesSync { 
        account_id, 
        note_ids, 
        response_tx: tx 
    }) {
        return code;
    }
    
    match rx.recv_timeout(SYNC_TIMEOUT) {
        Ok(Ok(tx_id_hex)) => {
            let out_capacity = unsafe { *tx_id_out_len };
            if tx_id_hex.len() > out_capacity {
                return ERR_INVALID_PARAM;
            }
            let out = unsafe { std::slice::from_raw_parts_mut(tx_id_out, tx_id_hex.len()) };
            out.copy_from_slice(tx_id_hex.as_bytes());
            unsafe { *tx_id_out_len = tx_id_hex.len() };
            0
        }
        Ok(Err(code)) => code,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => ERR_TIMEOUT,
        Err(_) => ERR_INVALID_HANDLE,
    }
}

// ================================================================================================
// FFI Interface - Async Operations (Non-blocking, callback-based)
// ================================================================================================

/// Sync state (async)
/// 
/// NOTE: Callback is invoked on worker thread, NOT main thread.
/// Swift callers should dispatch to main queue if updating UI.
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_sync_async(
    handle: MidenHandle,
    callback: SyncCallback,
    user_data: *mut std::ffi::c_void,
) -> i32 {
    let Some(worker) = get_handle(handle) else {
        return ERR_INVALID_HANDLE;
    };
    
    if let Err(code) = try_send_request(&worker.sender, Request::SyncAsync { 
        callback, 
        user_data: user_data as usize 
    }) {
        return code;
    }
    
    0
}

/// Create wallet (async)
/// 
/// NOTE: Callback is invoked on worker thread, NOT main thread.
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_create_wallet_async(
    handle: MidenHandle,
    seed_ptr: *const u8,
    seed_len: usize,
    callback: CreateWalletCallback,
    user_data: *mut std::ffi::c_void,
) -> i32 {
    let Some(worker) = get_handle(handle) else {
        return ERR_INVALID_HANDLE;
    };

    let seed: [u8; 32] = if seed_ptr.is_null() {
        let mut s = [0u8; 32];
        let mut rng = StdRng::from_os_rng();
        rng.fill_bytes(&mut s);
        s
    } else {
        if seed_len != 32 {
            return ERR_INVALID_PARAM;
        }
        let slice = unsafe { std::slice::from_raw_parts(seed_ptr, 32) };
        let mut arr = [0u8; 32];
        arr.copy_from_slice(slice);
        arr
    };

    if let Err(code) = try_send_request(&worker.sender, Request::CreateWalletAsync { 
        seed, 
        callback, 
        user_data: user_data as usize 
    }) {
        return code;
    }
    
    0
}

/// Get accounts (async)
/// 
/// NOTE: Callback is invoked on worker thread, NOT main thread.
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_get_accounts_async(
    handle: MidenHandle,
    callback: GetAccountsCallback,
    user_data: *mut std::ffi::c_void,
) -> i32 {
    let Some(worker) = get_handle(handle) else {
        return ERR_INVALID_HANDLE;
    };
    
    if let Err(code) = try_send_request(&worker.sender, Request::GetAccountsAsync { 
        callback, 
        user_data: user_data as usize 
    }) {
        return code;
    }
    
    0
}

/// Get balance (async)
/// 
/// NOTE: Callback is invoked on worker thread, NOT main thread.
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_get_balance_async(
    handle: MidenHandle,
    account_id_hex: *const c_char,
    callback: GetBalanceCallback,
    user_data: *mut std::ffi::c_void,
) -> i32 {
    let Some(worker) = get_handle(handle) else {
        return ERR_INVALID_HANDLE;
    };

    let (account_id, account_id_str) = match parse_account_id(account_id_hex) {
        Ok(v) => v,
        Err(code) => return code,
    };

    if let Err(code) = try_send_request(&worker.sender, Request::GetBalanceAsync { 
        account_id, 
        account_id_str, 
        callback, 
        user_data: user_data as usize 
    }) {
        return code;
    }
    
    0
}

/// Test connection (async)
/// 
/// NOTE: Callback is invoked on worker thread, NOT main thread.
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_test_connection_async(
    handle: MidenHandle,
    callback: TestConnectionCallback,
    user_data: *mut std::ffi::c_void,
) -> i32 {
    let Some(worker) = get_handle(handle) else {
        return ERR_INVALID_HANDLE;
    };
    
    if let Err(code) = try_send_request(&worker.sender, Request::TestConnectionAsync { 
        callback, 
        user_data: user_data as usize 
    }) {
        return code;
    }
    
    0
}

/// Get input notes (async)
/// 
/// NOTE: Callback is invoked on worker thread, NOT main thread.
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_get_input_notes_async(
    handle: MidenHandle,
    account_id_hex: *const c_char,
    callback: GetInputNotesCallback,
    user_data: *mut std::ffi::c_void,
) -> i32 {
    let Some(worker) = get_handle(handle) else {
        return ERR_INVALID_HANDLE;
    };

    let account_id = match parse_optional_account_id(account_id_hex) {
        Ok(v) => v,
        Err(code) => return code,
    };

    if let Err(code) = try_send_request(&worker.sender, Request::GetInputNotesAsync { 
        account_id, 
        callback, 
        user_data: user_data as usize 
    }) {
        return code;
    }
    
    0
}

/// Consume notes (async)
/// 
/// NOTE: Callback is invoked on worker thread, NOT main thread.
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_consume_notes_async(
    handle: MidenHandle,
    account_id_hex: *const c_char,
    note_ids_json: *const c_char,
    callback: ConsumeNotesCallback,
    user_data: *mut std::ffi::c_void,
) -> i32 {
    let Some(worker) = get_handle(handle) else {
        return ERR_INVALID_HANDLE;
    };

    if note_ids_json.is_null() {
        return ERR_INVALID_PARAM;
    }

    let (account_id, _) = match parse_account_id(account_id_hex) {
        Ok(v) => v,
        Err(code) => return code,
    };

    let note_ids_str = match unsafe { CStr::from_ptr(note_ids_json) }.to_str() {
        Ok(s) => s,
        Err(_) => return ERR_INVALID_PARAM,
    };

    let note_ids = match parse_note_ids_json(note_ids_str) {
        Ok(ids) if !ids.is_empty() => ids,
        _ => return ERR_NOTE_OP,
    };

    if let Err(code) = try_send_request(&worker.sender, Request::ConsumeNotesAsync { 
        account_id, 
        note_ids, 
        callback, 
        user_data: user_data as usize 
    }) {
        return code;
    }
    
    0
}

// ================================================================================================
// Utility Functions
// ================================================================================================

/// Keccak256 hash function
/// 
/// # Parameters
/// - `data_ptr`: Input data pointer
/// - `data_len`: Input data length
/// - `out_ptr`: Output buffer pointer (must be at least 32 bytes)
/// - `out_len`: Input: buffer capacity; Output: actual length (32)
/// 
/// # Returns
/// - 0: Success
/// - -1: Invalid parameters or buffer too small
#[unsafe(no_mangle)]
pub extern "C" fn wc_keccak256(
    data_ptr: *const u8,
    data_len: usize,
    out_ptr: *mut u8,
    out_len: *mut usize,
) -> i32 {
    if data_ptr.is_null() || out_ptr.is_null() || out_len.is_null() {
        return ERR_INVALID_PARAM;
    }
    
    // Check buffer capacity
    let capacity = unsafe { *out_len };
    if capacity < 32 {
        return ERR_INVALID_PARAM;
    }
    
    let data = unsafe { std::slice::from_raw_parts(data_ptr, data_len) };

    let mut hasher = Keccak256::new();
    hasher.update(data);
    let result = hasher.finalize();

    let out = unsafe { std::slice::from_raw_parts_mut(out_ptr, 32) };
    out.copy_from_slice(&result[..]);
    unsafe { *out_len = 32 };
    0
}

/// Convert account ID bytes to hex string
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_account_id_to_hex(
    account_id_ptr: *const u8,
    account_id_len: usize,
    hex_out: *mut u8,
    hex_out_len: *mut usize,
) -> i32 {
    if account_id_ptr.is_null() || hex_out.is_null() || hex_out_len.is_null() {
        return ERR_INVALID_PARAM;
    }

    let account_id_bytes = unsafe { std::slice::from_raw_parts(account_id_ptr, account_id_len) };
    let hex_string = hex::encode(account_id_bytes);
    
    let out_capacity = unsafe { *hex_out_len };
    if hex_string.len() > out_capacity {
        return ERR_INVALID_PARAM;
    }

    let out = unsafe { std::slice::from_raw_parts_mut(hex_out, hex_string.len()) };
    out.copy_from_slice(hex_string.as_bytes());
    unsafe { *hex_out_len = hex_string.len() };

    0
}
