#![allow(private_interfaces)]

// ================================================================================================
// Module Declarations
// ================================================================================================

mod runtime;
mod types;
mod memory;
mod client;
mod wallet;
mod account;
mod notes;
mod hash;

// ================================================================================================
// Re-export Public Types and FFI Functions
// ================================================================================================

// Re-export types for use in other modules
pub use types::{
    MidenHandle,
    MidenContext,
    MidenClient,
    MidenKeyStore,
    SyncCallback,
    CreateWalletCallback,
    GetAccountsCallback,
    GetBalanceCallback,
    GetInputNotesCallback,
    ConsumeNotesCallback,
    TestConnectionCallback,
};

// Re-export runtime utilities (internal use)
pub use runtime::{block_on, get_runtime};

// Re-export memory management FFI
pub use memory::wc_bytes_free;

// Re-export client FFI functions
pub use client::{
    wc_miden_create,
    wc_miden_destroy,
    wc_miden_sync,
    wc_miden_sync_async,
    wc_miden_test_connection,
    wc_miden_test_connection_async,
};

// Re-export wallet FFI functions
pub use wallet::{
    wc_miden_create_wallet,
    wc_miden_create_wallet_async,
};

// Re-export account FFI functions
pub use account::{
    wc_miden_get_accounts,
    wc_miden_get_accounts_async,
    wc_miden_get_balance,
    wc_miden_get_balance_async,
};

// Re-export notes FFI functions
pub use notes::{
    wc_miden_get_input_notes,
    wc_miden_get_input_notes_async,
    wc_miden_consume_notes,
    wc_miden_consume_notes_async,
};

// Re-export hash FFI functions
pub use hash::{
    wc_keccak256,
    wc_miden_account_id_to_hex,
};
