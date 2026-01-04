/// Free bytes allocated by Rust (for async callback results)
/// 
/// Swift must call this to free data returned via async callbacks
#[unsafe(no_mangle)]
pub extern "C" fn wc_bytes_free(ptr: *mut u8, len: usize) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        drop(Vec::from_raw_parts(ptr, len, len));
    }
}
