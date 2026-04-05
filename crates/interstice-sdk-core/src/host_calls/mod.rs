mod audio;
mod core;
mod file;
mod gpu;
mod module;

pub use audio::*;
pub use core::*;
pub use file::*;
pub use gpu::*;
use interstice_abi::{HostCall, decode, encode, unpack_ptr_len};
pub use module::*;
use serde::Deserialize;

#[link(wasm_import_module = "interstice")]
unsafe extern "C" {
    fn interstice_host_call(ptr: i32, len: i32) -> i64;

    // Direct hot-path host functions — no HostCall enum encode/decode overhead.
    pub fn interstice_log(msg_ptr: i32, msg_len: i32);
    pub fn interstice_time() -> i64;
    pub fn interstice_random() -> i64;
    /// insert_row: writes InsertRowResponse (bincode) into resp_ptr[0..resp_cap].
    /// Returns bytes written (>0) on success, negative required size on buffer-too-small.
    pub fn interstice_insert_row(
        table_ptr: i32,
        table_len: i32,
        row_ptr: i32,
        row_len: i32,
        resp_ptr: i32,
        resp_cap: i32,
    ) -> i32;
    /// Returns 0 on success, -1 on error.
    pub fn interstice_update_row(table_ptr: i32, table_len: i32, row_ptr: i32, row_len: i32)
    -> i32;
    /// Returns 0 on success, -1 on error.
    pub fn interstice_delete_row(table_ptr: i32, table_len: i32, pk_ptr: i32, pk_len: i32) -> i32;
    /// Clears the table in the current module. Returns 0 on success, -1 on error.
    pub fn interstice_clear_table(table_ptr: i32, table_len: i32) -> i32;
    /// get_by_pk: writes TableGetByPrimaryKeyResponse (bincode) into resp_ptr[0..resp_cap].
    /// Returns bytes written (>0) on success, negative required size on buffer-too-small.
    pub fn interstice_get_by_pk(
        table_ptr: i32,
        table_len: i32,
        pk_ptr: i32,
        pk_len: i32,
        resp_ptr: i32,
        resp_cap: i32,
    ) -> i32;
}

// Pre-allocated response buffer for direct host function calls.
// WASM is single-threaded so a module-level static is safe.
static mut DIRECT_RESP_BUF: [u8; 8192] = [0u8; 8192];

pub fn host_call(call: HostCall) -> i64 {
    let bytes = encode(&call).unwrap();

    return unsafe { interstice_host_call(bytes.as_ptr() as i32, bytes.len() as i32) };
}

fn unpack<T>(pack: i64) -> T
where
    T: for<'a> Deserialize<'a>,
{
    let (ptr, len) = unpack_ptr_len(pack);
    let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
    let result: T = decode(bytes).unwrap();
    return result;
}
