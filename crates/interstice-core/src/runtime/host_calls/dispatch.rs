use crate::runtime::wasm::{StoreState, read_bytes};
use crate::{error::IntersticeError, runtime::Runtime};
use interstice_abi::{Authority, HostCall, ModuleSchema, decode, encode, pack_ptr_len};
use serde::Serialize;
use wasmtime::{Caller, Memory};

impl Runtime {
    pub(crate) fn dispatch_host_call(
        &self,
        memory: &wasmtime::Memory,
        caller: &mut Caller<'_, StoreState>,
        caller_module_schema: ModuleSchema,
        ptr: i32,
        len: i32,
    ) -> Result<Option<i64>, IntersticeError> {
        let bytes = read_bytes(memory, caller, ptr, len)?;
        let host_call: HostCall = decode(&bytes).unwrap();

        return match host_call {
            HostCall::CallReducer(call_reducer_request) => {
                let response = self.handle_call_reducer(
                    &caller_module_schema.name.clone(),
                    call_reducer_request,
                )?;
                let result = self.send_data_to_module(response, memory, caller);
                Ok(Some(result))
            }
            HostCall::Log(log_request) => {
                self.handle_log(caller_module_schema.name.clone(), log_request);
                Ok(None)
            }
            HostCall::InsertRow(insert_row_request) => {
                let response =
                    self.handle_insert_row(&caller_module_schema.clone(), insert_row_request);
                let result = self.send_data_to_module(response, memory, caller);
                Ok(Some(result))
            }
            HostCall::UpdateRow(update_row_request) => {
                let response =
                    self.handle_update_row(caller_module_schema.name.clone(), update_row_request);
                let result = self.send_data_to_module(response, memory, caller);
                Ok(Some(result))
            }
            HostCall::DeleteRow(delete_row_request) => {
                let response =
                    self.handle_delete_row(caller_module_schema.name.clone(), delete_row_request);
                let result = self.send_data_to_module(response, memory, caller);
                Ok(Some(result))
            }
            HostCall::TableScan(table_scan_request) => {
                let response = self.handle_table_scan(table_scan_request);
                let result = self.send_data_to_module(response, memory, caller);
                Ok(Some(result))
            }
            HostCall::Gpu(gpu_call) => {
                let auth_modules = self.authority_modules.lock().unwrap();
                let gpu_auth_entry = auth_modules
                    .get(&Authority::Gpu)
                    .ok_or_else(|| IntersticeError::Internal("No GPU authority module".into()))?;

                if gpu_auth_entry.module_name != caller_module_schema.name {
                    return Err(IntersticeError::Unauthorized(Authority::Gpu));
                }

                drop(auth_modules);

                self.handle_gpu_call(gpu_call, memory, caller)
            }

            HostCall::Audio => todo!(),
            HostCall::Input => todo!(),
            HostCall::File(file_call) => {
                let auth_modules = self.authority_modules.lock().unwrap();
                let file_auth_entry = auth_modules
                    .get(&Authority::File)
                    .ok_or_else(|| IntersticeError::Internal("No File authority module".into()))?;

                if file_auth_entry.module_name != caller_module_schema.name {
                    return Err(IntersticeError::Unauthorized(Authority::File));
                }

                drop(auth_modules);

                self.handle_file_call(file_call, memory, caller)
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

    pub fn send_data_to_module<T>(
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
