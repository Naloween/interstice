pub mod internal;
pub mod macros;
mod registry;

#[link(wasm_import_module = "interstice")]
unsafe extern "C" {
    fn interstice_host_call(ptr: i32, len: i32) -> i64;
}

pub use interstice_sdk_core;
pub use interstice_sdk_macros::*;
