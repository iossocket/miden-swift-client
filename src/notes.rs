use std::ffi::CStr;
use std::os::raw::c_char;
use miden_objects::account::AccountId;
use miden_objects::note::NoteId;
use miden_client::transaction::TransactionRequestBuilder;

use crate::runtime::block_on;
use crate::types::{MidenClient, MidenHandle, GetInputNotesCallback, ConsumeNotesCallback};

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
        let ctx = context.lock().await;
        ctx.client.get_consumable_notes(account_id).await
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

/// Async version of get input notes
/// 
/// # Parameters
/// - `handle`: Client handle
/// - `account_id_hex`: Account ID (hex string, can be NULL for all accounts)
/// - `callback`: Callback function (user_data, error_code, json_ptr, json_len)
/// - `user_data`: User data passed to callback
/// 
/// # Returns
/// - 0: Task started successfully
/// - -1: Invalid parameters
/// - -2: Invalid handle
/// - -3: Account ID parsing failed
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_get_input_notes_async(
    handle: MidenHandle,
    account_id_hex: *const c_char,
    callback: GetInputNotesCallback,
    user_data: *mut std::ffi::c_void,
) -> i32 {
    if handle.is_null() {
        return -2;
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

    let handle_usize = handle as usize;
    let user_data_usize = user_data as usize;

    std::thread::spawn(move || {
        let handle = handle_usize as MidenHandle;
        let context = unsafe { &*handle };
        
        let result = block_on(async {
            let ctx = context.lock().await;
            ctx.client.get_consumable_notes(account_id).await
        });

        let user_data_ptr = user_data_usize as *mut std::ffi::c_void;
        match result {
            Ok(consumable_notes) => {
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

                // Allocate stable memory - Swift must call wc_bytes_free
                let mut bytes = json.into_bytes();
                let ptr = bytes.as_mut_ptr();
                let len = bytes.len();
                std::mem::forget(bytes);
                callback(user_data_ptr, 0, ptr, len);
            }
            Err(_) => {
                callback(user_data_ptr, -4, std::ptr::null_mut(), 0);
            }
        }
    });

    0
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

    let context = unsafe { &*handle };

    // Build and submit transaction
    let result = block_on(async {
        let mut ctx = context.lock().await;
        consume_notes_async(&mut ctx.client, account_id, note_ids).await
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

/// Async version of consume notes
/// 
/// # Parameters
/// - `handle`: Client handle
/// - `account_id_hex`: Account ID to execute transaction (hex string)
/// - `note_ids_json`: JSON-formatted array of note IDs
/// - `callback`: Callback function (user_data, error_code, tx_id_ptr, tx_id_len)
/// - `user_data`: User data passed to callback
/// 
/// # Returns
/// - 0: Task started successfully
/// - -1: Invalid parameters
/// - -2: Invalid handle
/// - -3: Account ID parsing failed
/// - -4: Note IDs parsing failed
#[unsafe(no_mangle)]
pub extern "C" fn wc_miden_consume_notes_async(
    handle: MidenHandle,
    account_id_hex: *const c_char,
    note_ids_json: *const c_char,
    callback: ConsumeNotesCallback,
    user_data: *mut std::ffi::c_void,
) -> i32 {
    if handle.is_null() {
        return -2;
    }
    if account_id_hex.is_null() || note_ids_json.is_null() {
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

    let note_ids: Vec<NoteId> = match parse_note_ids_json(note_ids_str) {
        Ok(ids) => ids,
        Err(_) => return -4,
    };

    if note_ids.is_empty() {
        return -4;
    }

    let handle_usize = handle as usize;
    let user_data_usize = user_data as usize;

    std::thread::spawn(move || {
        let handle = handle_usize as MidenHandle;
        let context = unsafe { &*handle };
        
        let result = block_on(async {
            let mut ctx = context.lock().await;
            consume_notes_async(&mut ctx.client, account_id, note_ids).await
        });

        let user_data_ptr = user_data_usize as *mut std::ffi::c_void;
        match result {
            Ok(tx_id_hex) => {
                // Allocate stable memory - Swift must call wc_bytes_free
                let mut bytes = tx_id_hex.into_bytes();
                let ptr = bytes.as_mut_ptr();
                let len = bytes.len();
                std::mem::forget(bytes);
                callback(user_data_ptr, 0, ptr, len);
            }
            Err(e) => {
                let error_code = if e.contains("request") || e.contains("build") {
                    -5
                } else {
                    -6
                };
                callback(user_data_ptr, error_code, std::ptr::null_mut(), 0);
            }
        }
    });

    0
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
pub async fn consume_notes_async(
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
