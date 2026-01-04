use std::ffi::CStr;
use std::os::raw::c_char;
use miden_objects::account::AccountId;

use crate::runtime::block_on;
use crate::types::{MidenHandle, GetAccountsCallback, GetBalanceCallback};

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
        let ctx = context.lock().await;
        ctx.client.get_account_headers().await
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

/// Async version of get accounts
/// 
/// # Parameters
/// - `handle`: Client handle
/// - `callback`: Callback function (user_data, error_code, json_ptr, json_len)
/// - `user_data`: User data passed to callback
/// 
/// # Returns
/// - 0: Task started successfully
/// - -1: Invalid handle
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_get_accounts_async(
    handle: MidenHandle,
    callback: GetAccountsCallback,
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
            let ctx = context.lock().await;
            ctx.client.get_account_headers().await
        });

        let user_data_ptr = user_data_usize as *mut std::ffi::c_void;
        match result {
            Ok(accounts) => {
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

                // Allocate stable memory - Swift must call wc_bytes_free
                let mut bytes = json.into_bytes();
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
        let ctx = context.lock().await;
        ctx.client.get_account(account_id).await
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

/// Async version of get balance
/// 
/// # Parameters
/// - `handle`: Client handle
/// - `account_id_hex`: Account ID (hex string)
/// - `callback`: Callback function (user_data, error_code, json_ptr, json_len)
/// - `user_data`: User data passed to callback
/// 
/// # Returns
/// - 0: Task started successfully
/// - -1: Invalid parameters
/// - -2: Invalid handle
/// - -3: Account ID parsing failed
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_get_balance_async(
    handle: MidenHandle,
    account_id_hex: *const c_char,
    callback: GetBalanceCallback,
    user_data: *mut std::ffi::c_void,
) -> i32 {
    if handle.is_null() {
        return -2;
    }
    if account_id_hex.is_null() {
        return -1;
    }

    // Parse account ID
    let account_id_str = match unsafe { CStr::from_ptr(account_id_hex) }.to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return -1,
    };

    let account_id = match AccountId::from_hex(&account_id_str) {
        Ok(id) => id,
        Err(_) => return -3,
    };

    let handle_usize = handle as usize;
    let user_data_usize = user_data as usize;

    std::thread::spawn(move || {
        let handle = handle_usize as MidenHandle;
        let context = unsafe { &*handle };
        
        let result = block_on(async {
            let ctx = context.lock().await;
            ctx.client.get_account(account_id).await
        });

        let user_data_ptr = user_data_usize as *mut std::ffi::c_void;
        match result {
            Ok(Some(account_record)) => {
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

                // Allocate stable memory - Swift must call wc_bytes_free
                let mut bytes = json.into_bytes();
                let ptr = bytes.as_mut_ptr();
                let len = bytes.len();
                std::mem::forget(bytes);
                callback(user_data_ptr, 0, ptr, len);
            }
            Ok(None) => {
                callback(user_data_ptr, -4, std::ptr::null_mut(), 0);
            }
            Err(_) => {
                callback(user_data_ptr, -5, std::ptr::null_mut(), 0);
            }
        }
    });

    0
}
