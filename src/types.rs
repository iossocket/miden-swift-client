use std::sync::Arc;
use tokio::sync::Mutex;
use miden_client::{
    keystore::FilesystemKeyStore,
    Client,
};
use rand::rngs::StdRng;

/// Miden Client type aliases
pub type MidenKeyStore = FilesystemKeyStore<StdRng>;
pub type MidenClient = Client<MidenKeyStore>;

/// Client context containing all required resources
pub struct MidenContext {
    pub client: MidenClient,
    pub keystore: Arc<MidenKeyStore>,
}

/// Opaque handle type
/// Points to Arc<Mutex<MidenContext>> for thread-safe access
pub type MidenHandle = *mut Arc<Mutex<MidenContext>>;

// ================================================================================================
// Async Callback Types
// ================================================================================================

/// Callback for sync operation: (user_data, error_code, block_num)
pub type SyncCallback = extern "C" fn(*mut std::ffi::c_void, i32, u32);

/// Callback for create wallet operation: (user_data, error_code, account_id_ptr, account_id_len)
/// Note: Swift must call wc_bytes_free(ptr, len) to free the returned data
pub type CreateWalletCallback = extern "C" fn(*mut std::ffi::c_void, i32, *mut u8, usize);

/// Callback for get accounts operation: (user_data, error_code, json_ptr, json_len)
/// Note: Swift must call wc_bytes_free(ptr, len) to free the returned data
pub type GetAccountsCallback = extern "C" fn(*mut std::ffi::c_void, i32, *mut u8, usize);

/// Callback for get balance operation: (user_data, error_code, json_ptr, json_len)
/// Note: Swift must call wc_bytes_free(ptr, len) to free the returned data
pub type GetBalanceCallback = extern "C" fn(*mut std::ffi::c_void, i32, *mut u8, usize);

/// Callback for get input notes operation: (user_data, error_code, json_ptr, json_len)
/// Note: Swift must call wc_bytes_free(ptr, len) to free the returned data
pub type GetInputNotesCallback = extern "C" fn(*mut std::ffi::c_void, i32, *mut u8, usize);

/// Callback for consume notes operation: (user_data, error_code, tx_id_ptr, tx_id_len)
/// Note: Swift must call wc_bytes_free(ptr, len) to free the returned data
pub type ConsumeNotesCallback = extern "C" fn(*mut std::ffi::c_void, i32, *mut u8, usize);

/// Callback for test connection operation: (user_data, error_code)
pub type TestConnectionCallback = extern "C" fn(*mut std::ffi::c_void, i32);
