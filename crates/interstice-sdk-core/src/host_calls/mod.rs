mod core;
mod gpu;

pub use core::*;
pub use gpu::*;
use interstice_abi::{HostCall, decode, encode, unpack_ptr_len};
use serde::Deserialize;

#[link(wasm_import_module = "interstice")]
unsafe extern "C" {
    fn interstice_host_call(ptr: i32, len: i32) -> i64;
}

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
