use std::{
    ffi::CStr,
    os::raw::c_char,
    path::PathBuf,
    sync::Arc,
};
use tokio::sync::Mutex;

use miden_client::{
    builder::ClientBuilder,
    keystore::FilesystemKeyStore,
    rpc::{Endpoint, GrpcClient},
};
use miden_client_sqlite_store::ClientBuilderSqliteExt;
use rand::rngs::StdRng;

use crate::runtime::block_on;
use crate::types::{MidenContext, MidenHandle, SyncCallback, TestConnectionCallback};

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
            let ctx = Arc::new(Mutex::new(context));
            let boxed = Box::new(ctx);
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

    let context = unsafe { &*handle };
    
    let result = block_on(async {
        let mut ctx = context.lock().await;
        ctx.client.sync_state().await
    });

    match result {
        Ok(summary) => {
            if !block_num_out.is_null() {
                unsafe { *block_num_out = summary.block_num.as_u32() };
            }
            0
        }
        Err(e) => {
            eprintln!("[wc_miden_sync] sync_state failed: {:?}", e);
            -2
        }
    }
}

/// Async version of sync state
/// 
/// # Parameters
/// - `handle`: Client handle
/// - `callback`: Callback function (user_data, error_code, block_num)
/// - `user_data`: User data passed to callback
/// 
/// # Returns
/// - 0: Task started successfully
/// - -1: Invalid handle or callback
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_sync_async(
    handle: MidenHandle,
    callback: SyncCallback,
    user_data: *mut std::ffi::c_void,
) -> i32 {
    if handle.is_null() {
        return -1;
    }

    // Convert handle to usize for thread transfer (caller must ensure handle remains valid)
    let handle_usize = handle as usize;
    let user_data_usize = user_data as usize;

    std::thread::spawn(move || {
        let handle = handle_usize as MidenHandle;
        let context = unsafe { &*handle };
        
        let result = block_on(async {
            let mut ctx = context.lock().await;
            ctx.client.sync_state().await
        });

        let user_data_ptr = user_data_usize as *mut std::ffi::c_void;
        match result {
            Ok(summary) => {
                callback(user_data_ptr, 0, summary.block_num.as_u32());
            }
            Err(_) => {
                callback(user_data_ptr, -2, 0);
            }
        }
    });

    0
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

    let context = unsafe { &*handle };
    
    let result = block_on(async {
        let mut ctx = context.lock().await;
        ctx.client.sync_state().await
    });

    match result {
        Ok(_) => 0,
        Err(_) => -2,
    }
}

/// Async version of test connection
/// 
/// # Parameters
/// - `handle`: Client handle
/// - `callback`: Callback function (user_data, error_code)
/// - `user_data`: User data passed to callback
/// 
/// # Returns
/// - 0: Task started successfully
/// - -1: Invalid handle
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_test_connection_async(
    handle: MidenHandle,
    callback: TestConnectionCallback,
    user_data: *mut std::ffi::c_void,
) -> i32 {
    if handle.is_null() {
        return -1;
    }

    let handle_usize = handle as usize;
    let user_data_usize = user_data as usize;

    std::thread::spawn(move || {
        let handle = handle_usize as MidenHandle;
        let context = unsafe { &*handle };
        
        let result = block_on(async {
            let mut ctx = context.lock().await;
            ctx.client.sync_state().await
        });

        let user_data_ptr = user_data_usize as *mut std::ffi::c_void;
        match result {
            Ok(_) => callback(user_data_ptr, 0),
            Err(_) => callback(user_data_ptr, -2),
        }
    });

    0
}
