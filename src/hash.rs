use sha3::{Digest, Keccak256};

// pub fn keccak256_bytes(data: &[u8]) -> [u8; 32] {
//     let mut hasher = Keccak256::new();
//     hasher.update(data);
//     let out = hasher.finalize();
    
//     let mut arr = [0u8; 32];
//     arr.copy_from_slice(&out);
//     arr
// }

// pub fn keccak256_bytes_v2(data: &[u8]) -> [u8; 32] {
//     let mut hasher = Keccak256::new();
//     hasher.update(data);
//     let out = hasher.finalize();
//     out.into()
// }

// pub fn keccak256_bytes_v3(data: &[u8]) -> [u8; 32] {
//     Keccak256::digest(data).into()
// }

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
