use std::sync::Arc;
use rand::{rngs::StdRng, RngCore, SeedableRng};

use miden_client::{
    account::component::BasicWallet,
    auth::AuthSecretKey,
};
use miden_lib::account::auth::AuthRpoFalcon512;
use miden_objects::account::{
    Account, AccountBuilder, AccountComponent, AccountStorageMode, AccountType,
};

use crate::runtime::block_on;
use crate::types::{MidenClient, MidenHandle, MidenKeyStore, CreateWalletCallback};

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

    let context = unsafe { &*handle };
    
    let result = block_on(async {
        let mut ctx = context.lock().await;
        let keystore = ctx.keystore.clone();
        create_wallet_async(&mut ctx.client, &keystore, init_seed).await
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

/// Async version of create wallet
/// 
/// # Parameters
/// - `handle`: Client handle
/// - `seed_ptr`: 32-byte random seed (if NULL, auto-generated)
/// - `seed_len`: Seed length (must be 32, ignored if seed_ptr is NULL)
/// - `callback`: Callback function (user_data, error_code, account_id_ptr, account_id_len)
/// - `user_data`: User data passed to callback
/// 
/// # Returns
/// - 0: Task started successfully
/// - -1: Invalid parameters
/// - -2: Invalid handle
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_create_wallet_async(
    handle: MidenHandle,
    seed_ptr: *const u8,
    seed_len: usize,
    callback: CreateWalletCallback,
    user_data: *mut std::ffi::c_void,
) -> i32 {
    if handle.is_null() {
        return -2;
    }

    // Get or generate seed
    let init_seed: [u8; 32] = if seed_ptr.is_null() {
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

    let handle_usize = handle as usize;
    let user_data_usize = user_data as usize;

    std::thread::spawn(move || {
        let handle = handle_usize as MidenHandle;
        let context = unsafe { &*handle };
        
        let result = block_on(async {
            let mut ctx = context.lock().await;
            let keystore = ctx.keystore.clone();
            create_wallet_async(&mut ctx.client, &keystore, init_seed).await
        });

        let user_data_ptr = user_data_usize as *mut std::ffi::c_void;
        match result {
            Ok(account) => {
                let account_id_hex = account.id().to_hex();
                // Allocate stable memory - Swift must call wc_bytes_free
                let mut bytes = account_id_hex.into_bytes();
                let ptr = bytes.as_mut_ptr();
                let len = bytes.len();
                std::mem::forget(bytes);
                callback(user_data_ptr, 0, ptr, len);
            }
            Err(_) => {
                callback(user_data_ptr, -3, std::ptr::null_mut(), 0);
            }
        }
    });

    0
}

/// Asynchronously create wallet
pub async fn create_wallet_async(
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
