use crate::{
    runtime::Runtime,
    wasm::{StoreState, read_bytes},
};
use interstice_abi::{decode, host::HostCall, types::ModuleId};
use wasmtime::Caller;

impl Runtime {
    pub fn dispatch_host_call(
        &mut self,
        caller_module: ModuleId,
        memory: &wasmtime::Memory,
        caller: &mut Caller<'_, StoreState>,
        ptr: i32,
        len: i32,
    ) -> i64 {
        let bytes = match read_bytes(memory, caller, ptr, len) {
            Ok(b) => b,
            Err(_) => return 0,
        };

        let host_call: HostCall = decode(&bytes).unwrap();

        match host_call {
            HostCall::CallReducer(call_reducer_request) => {
                self.handle_call_reducer(memory, caller, call_reducer_request)
            }
            HostCall::Log(log_request) => {
                self.handle_log(caller_module, log_request);
                0
            }
            HostCall::Abort(abort_request) => {
                self.handle_abort(abort_request);
                unreachable!()
            }
            _ => 0,
        }
    }
}
