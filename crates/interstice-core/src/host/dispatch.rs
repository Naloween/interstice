use crate::{
    error::IntersticeError,
    runtime::Runtime,
    wasm::{StoreState, read_bytes},
};
use interstice_abi::{codec::pack_ptr_len, decode, encode, host::HostCall};
use serde::Serialize;
use wasmtime::{Caller, Memory};

impl Runtime {
    pub(crate) fn dispatch_host_call(
        &mut self,
        memory: &wasmtime::Memory,
        caller: &mut Caller<'_, StoreState>,
        caller_module_name: String,
        ptr: i32,
        len: i32,
    ) -> Result<Option<i64>, IntersticeError> {
        let bytes = read_bytes(memory, caller, ptr, len)?;
        let host_call: HostCall = decode(&bytes).unwrap();

        return match host_call {
            HostCall::CallReducer(call_reducer_request) => {
                let response = self.handle_call_reducer(call_reducer_request)?;
                let result = self.send_data_to_module(response, memory, caller);
                Ok(Some(result))
            }
            HostCall::Log(log_request) => {
                self.handle_log(caller_module_name, log_request);
                Ok(None)
            }
            HostCall::Abort(abort_request) => {
                self.handle_abort(abort_request);
                Ok(None)
            }
            HostCall::InsertRow(insert_row_request) => {
                let response = self.handle_insert_row(insert_row_request);
                let result = self.send_data_to_module(response, memory, caller);
                Ok(Some(result))
            }
            HostCall::UpdateRow(update_row_request) => {
                let response = self.handle_update_row(update_row_request);
                let result = self.send_data_to_module(response, memory, caller);
                Ok(Some(result))
            }
            HostCall::DeleteRow(delete_row_request) => {
                let response = self.handle_delete_row(delete_row_request);
                let result = self.send_data_to_module(response, memory, caller);
                Ok(Some(result))
            }
            HostCall::TableScan(table_scan_request) => {
                let response = self.handle_table_scan(table_scan_request);
                let result = self.send_data_to_module(response, memory, caller);
                Ok(Some(result))
            }
        };
    }

    fn send_bytes_to_module(
        &self,
        memory: &Memory,
        mut caller: &mut Caller<'_, StoreState>,
        bytes: &[u8],
    ) -> (i32, i32) {
        let alloc = caller
            .get_export("alloc")
            .unwrap()
            .into_func()
            .unwrap()
            .typed::<i32, i32>(&caller)
            .unwrap();

        let ptr = alloc.call(&mut caller, bytes.len() as i32).unwrap();

        memory.write(&mut caller, ptr as usize, bytes).unwrap();

        (ptr, bytes.len() as i32)
    }

    fn send_data_to_module<T>(
        &self,
        result: T,
        memory: &wasmtime::Memory,
        caller: &mut Caller<'_, StoreState>,
    ) -> i64
    where
        T: Serialize,
    {
        let encoded = encode(&result).unwrap();
        let (ptr, len) = self.send_bytes_to_module(memory, caller, &encoded);
        return pack_ptr_len(ptr, len);
    }
}
