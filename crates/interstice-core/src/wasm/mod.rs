pub mod instance;
pub mod linker;

use wasmtime::{Caller, Memory};

use crate::{Node, error::IntersticeError};

pub struct StoreState {
    pub node: *mut Node,
    pub module_name: String,
}

pub fn read_bytes(
    memory: &Memory,
    caller: &mut Caller<StoreState>,
    ptr: i32,
    len: i32,
) -> Result<Vec<u8>, IntersticeError> {
    let mut out = vec![0u8; len as usize];
    memory
        .read(caller, ptr as usize, &mut out)
        .map_err(|_| IntersticeError::MemoryRead)?;
    return Ok(out);
}
