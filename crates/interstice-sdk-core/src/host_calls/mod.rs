mod core;
mod gpu;

pub use core::*;
pub use gpu::*;
use interstice_abi::{HostCall, encode};

#[link(wasm_import_module = "interstice")]
unsafe extern "C" {
    fn interstice_host_call(ptr: i32, len: i32) -> i64;
}

pub fn host_call(call: HostCall) -> i64 {
    let bytes = encode(&call).unwrap();

    return unsafe { interstice_host_call(bytes.as_ptr() as i32, bytes.len() as i32) };
}
