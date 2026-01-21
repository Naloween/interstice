use crate::{
    runtime::Runtime,
    wasm::{StoreState, read_bytes},
};
use interstice_abi::{host_calls, types::ModuleId};
use wasmtime::Caller;

impl Runtime {
    pub fn dispatch_host_call(
        &mut self,
        caller_module: ModuleId,
        memory: &wasmtime::Memory,
        caller: &mut Caller<'_, StoreState>,
        call_id: u32,
        ptr: i32,
        len: i32,
    ) -> i64 {
        let bytes = match read_bytes(memory, caller, ptr, len) {
            Ok(b) => b,
            Err(_) => return 0,
        };

        match call_id {
            host_calls::CALL_REDUCER => self.handle_call_reducer(memory, caller, &bytes),
            host_calls::LOG => {
                self.handle_log(caller_module, &bytes);
                0
            }
            host_calls::ABORT => {
                self.handle_abort(&bytes);
                unreachable!()
            }
            _ => 0,
        }
    }
}
