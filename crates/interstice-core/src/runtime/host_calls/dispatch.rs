use crate::runtime::wasm::{StoreState, read_bytes};
use crate::{error::IntersticeError, runtime::Runtime};
use interstice_abi::{
    Authority, CallQueryResponse, CallReducerResponse, HostCall, ModuleSchema, decode, encode,
    pack_ptr_len,
};
use serde::Serialize;
use wasmtime::{Caller, Memory};

impl Runtime {
    pub(crate) async fn dispatch_host_call(
        &self,
        memory: &wasmtime::Memory,
        caller: &mut Caller<'_, StoreState>,
        caller_module_schema: ModuleSchema,
        ptr: i32,
        len: i32,
    ) -> Result<Option<i64>, IntersticeError> {
        let bytes = read_bytes(memory, caller, ptr, len)?;
        let host_call: HostCall = decode(&bytes).map_err(|err| {
            IntersticeError::Internal(format!("Failed to decode host call: {err}"))
        })?;

        return match host_call {
            HostCall::CallReducer(call_reducer_request) => {
                let response = match self
                    .handle_call_reducer(&caller_module_schema.name.clone(), call_reducer_request)
                    .await
                {
                    Ok(()) => CallReducerResponse::Ok,
                    Err(err) => CallReducerResponse::Err(err.to_string()),
                };
                let result = self.send_data_to_module(response, memory, caller).await;
                Ok(Some(result))
            }
            HostCall::Schedule(schedule_request) => {
                let response =
                    self.handle_schedule(caller_module_schema.name.clone(), schedule_request);
                let result = self.send_data_to_module(response, memory, caller).await;
                Ok(Some(result))
            }
            HostCall::CallQuery(call_query_request) => {
                let response = match self
                    .handle_call_query(&caller_module_schema.name.clone(), call_query_request)
                    .await
                {
                    Ok(result) => CallQueryResponse::Ok(result),
                    Err(err) => CallQueryResponse::Err(err.to_string()),
                };
                let result = self.send_data_to_module(response, memory, caller).await;
                Ok(Some(result))
            }
            HostCall::DeterministicRandom(request) => {
                let response = self.handle_deterministic_random(request);
                let result = self.send_data_to_module(response, memory, caller).await;
                Ok(Some(result))
            }
            HostCall::Time(request) => {
                let response = self.handle_time(request);
                let result = self.send_data_to_module(response, memory, caller).await;
                Ok(Some(result))
            }
            HostCall::Log(log_request) => {
                self.handle_log(caller_module_schema.name.clone(), log_request);
                Ok(None)
            }
            HostCall::InsertRow(insert_row_request) => {
                let response =
                    self.handle_insert_row(&caller_module_schema.clone(), insert_row_request);
                let result = self.send_data_to_module(response, memory, caller).await;
                Ok(Some(result))
            }
            HostCall::UpdateRow(update_row_request) => {
                let response = self.handle_update_row(&caller_module_schema, update_row_request);
                let result = self.send_data_to_module(response, memory, caller).await;
                Ok(Some(result))
            }
            HostCall::DeleteRow(delete_row_request) => {
                let response =
                    self.handle_delete_row(caller_module_schema.name.clone(), delete_row_request);
                let result = self.send_data_to_module(response, memory, caller).await;
                Ok(Some(result))
            }
            HostCall::ClearTable(clear_table_request) => {
                let response =
                    self.handle_clear_table(caller_module_schema.name.clone(), clear_table_request);
                let result = self.send_data_to_module(response, memory, caller).await;
                Ok(Some(result))
            }
            HostCall::TableScan(table_scan_request) => {
                let response = self.handle_table_scan(table_scan_request);
                let result = self.send_data_to_module(response, memory, caller).await;
                Ok(Some(result))
            }
            HostCall::TableGetByPrimaryKey(request) => {
                let response = self.handle_table_get_by_primary_key(request);
                let result = self.send_data_to_module(response, memory, caller).await;
                Ok(Some(result))
            }
            HostCall::TableIndexScan(request) => {
                let response = self.handle_table_index_scan(request);
                let result = self.send_data_to_module(response, memory, caller).await;
                Ok(Some(result))
            }
            HostCall::Gpu(gpu_call) => {
                let gpu_auth_module = {
                    let auth_modules = self.authority_modules.lock().unwrap();
                    auth_modules
                        .get(&Authority::Gpu)
                        .map(|entry| entry.module_name().to_string())
                };

                match gpu_auth_module {
                    None => {
                        let response =
                            interstice_abi::GpuResponse::Err("No GPU authority module".into());
                        let result = self.send_data_to_module(response, memory, caller).await;
                        return Ok(Some(result));
                    }
                    Some(module_name) => {
                        if module_name != caller_module_schema.name {
                            let response = interstice_abi::GpuResponse::Err(
                                IntersticeError::Unauthorized(Authority::Gpu).to_string(),
                            );
                            let result = self.send_data_to_module(response, memory, caller).await;
                            return Ok(Some(result));
                        }
                    }
                }

                self.handle_gpu_call(gpu_call, memory, caller).await
            }
            HostCall::Audio(audio_call) => {
                let audio_auth_module = {
                    let auth_modules = self.authority_modules.lock().unwrap();
                    auth_modules
                        .get(&Authority::Audio)
                        .map(|entry| entry.module_name().to_string())
                };

                match audio_auth_module {
                    None => {
                        let response =
                            interstice_abi::AudioResponse::Err("No Audio authority module".into());
                        let result = self.send_data_to_module(response, memory, caller).await;
                        return Ok(Some(result));
                    }
                    Some(module_name) => {
                        if module_name != caller_module_schema.name {
                            let response = interstice_abi::AudioResponse::Err(
                                IntersticeError::Unauthorized(Authority::Audio).to_string(),
                            );
                            let result = self.send_data_to_module(response, memory, caller).await;
                            return Ok(Some(result));
                        }
                    }
                }

                let response = self.handle_audio_call(audio_call);
                let result = self.send_data_to_module(response, memory, caller).await;
                Ok(Some(result))
            }
            HostCall::Input => todo!(),
            HostCall::File(file_call) => {
                let file_auth_module = {
                    let auth_modules = self.authority_modules.lock().unwrap();
                    auth_modules
                        .get(&Authority::File)
                        .map(|entry| entry.module_name().to_string())
                };

                match file_auth_module {
                    None => {
                        let result = match &file_call {
                            interstice_abi::FileCall::ReadFile(_) => {
                                self.send_data_to_module(
                                    interstice_abi::ReadFileResponse::Err(
                                        "No File authority module".into(),
                                    ),
                                    memory,
                                    caller,
                                )
                                .await
                            }
                            interstice_abi::FileCall::WriteFile(_) => {
                                self.send_data_to_module(
                                    interstice_abi::WriteFileResponse::Err(
                                        "No File authority module".into(),
                                    ),
                                    memory,
                                    caller,
                                )
                                .await
                            }
                            interstice_abi::FileCall::ListDir(_) => {
                                self.send_data_to_module(
                                    interstice_abi::ListDirResponse::Err(
                                        "No File authority module".into(),
                                    ),
                                    memory,
                                    caller,
                                )
                                .await
                            }
                            interstice_abi::FileCall::Metadata(_) => {
                                self.send_data_to_module(
                                    interstice_abi::MetadataResponse::Err(
                                        "No File authority module".into(),
                                    ),
                                    memory,
                                    caller,
                                )
                                .await
                            }
                            interstice_abi::FileCall::CreateDir(_) => {
                                self.send_data_to_module(
                                    interstice_abi::CreateDirResponse::Err(
                                        "No File authority module".into(),
                                    ),
                                    memory,
                                    caller,
                                )
                                .await
                            }
                            interstice_abi::FileCall::RemoveFile(_) => {
                                self.send_data_to_module(
                                    interstice_abi::RemoveFileResponse::Err(
                                        "No File authority module".into(),
                                    ),
                                    memory,
                                    caller,
                                )
                                .await
                            }
                            interstice_abi::FileCall::RemoveDir(_) => {
                                self.send_data_to_module(
                                    interstice_abi::RemoveDirResponse::Err(
                                        "No File authority module".into(),
                                    ),
                                    memory,
                                    caller,
                                )
                                .await
                            }
                            interstice_abi::FileCall::Rename(_) => {
                                self.send_data_to_module(
                                    interstice_abi::RenameResponse::Err(
                                        "No File authority module".into(),
                                    ),
                                    memory,
                                    caller,
                                )
                                .await
                            }
                            interstice_abi::FileCall::Copy(_) => {
                                self.send_data_to_module(
                                    interstice_abi::CopyResponse::Err(
                                        "No File authority module".into(),
                                    ),
                                    memory,
                                    caller,
                                )
                                .await
                            }
                        };
                        return Ok(Some(result));
                    }
                    Some(module_name) => {
                        if module_name != caller_module_schema.name {
                            let err = IntersticeError::Unauthorized(Authority::File).to_string();
                            let result = match &file_call {
                                interstice_abi::FileCall::ReadFile(_) => {
                                    self.send_data_to_module(
                                        interstice_abi::ReadFileResponse::Err(err.clone()),
                                        memory,
                                        caller,
                                    )
                                    .await
                                }
                                interstice_abi::FileCall::WriteFile(_) => {
                                    self.send_data_to_module(
                                        interstice_abi::WriteFileResponse::Err(err.clone()),
                                        memory,
                                        caller,
                                    )
                                    .await
                                }
                                interstice_abi::FileCall::ListDir(_) => {
                                    self.send_data_to_module(
                                        interstice_abi::ListDirResponse::Err(err.clone()),
                                        memory,
                                        caller,
                                    )
                                    .await
                                }
                                interstice_abi::FileCall::Metadata(_) => {
                                    self.send_data_to_module(
                                        interstice_abi::MetadataResponse::Err(err.clone()),
                                        memory,
                                        caller,
                                    )
                                    .await
                                }
                                interstice_abi::FileCall::CreateDir(_) => {
                                    self.send_data_to_module(
                                        interstice_abi::CreateDirResponse::Err(err.clone()),
                                        memory,
                                        caller,
                                    )
                                    .await
                                }
                                interstice_abi::FileCall::RemoveFile(_) => {
                                    self.send_data_to_module(
                                        interstice_abi::RemoveFileResponse::Err(err.clone()),
                                        memory,
                                        caller,
                                    )
                                    .await
                                }
                                interstice_abi::FileCall::RemoveDir(_) => {
                                    self.send_data_to_module(
                                        interstice_abi::RemoveDirResponse::Err(err.clone()),
                                        memory,
                                        caller,
                                    )
                                    .await
                                }
                                interstice_abi::FileCall::Rename(_) => {
                                    self.send_data_to_module(
                                        interstice_abi::RenameResponse::Err(err.clone()),
                                        memory,
                                        caller,
                                    )
                                    .await
                                }
                                interstice_abi::FileCall::Copy(_) => {
                                    self.send_data_to_module(
                                        interstice_abi::CopyResponse::Err(err.clone()),
                                        memory,
                                        caller,
                                    )
                                    .await
                                }
                            };
                            return Ok(Some(result));
                        }
                    }
                }

                self.handle_file_call(file_call, memory, caller).await
            }
            HostCall::Module(module_call) => {
                let module_auth_module = {
                    let auth_modules = self.authority_modules.lock().unwrap();
                    auth_modules
                        .get(&Authority::Module)
                        .map(|entry| entry.module_name().to_string())
                };

                match module_auth_module {
                    None => {
                        let response = interstice_abi::ModuleCallResponse::Err(
                            "No Module authority module".into(),
                        );
                        let result = self.send_data_to_module(response, memory, caller).await;
                        return Ok(Some(result));
                    }
                    Some(module_name) => {
                        if module_name != caller_module_schema.name {
                            let response = interstice_abi::ModuleCallResponse::Err(
                                IntersticeError::Unauthorized(Authority::Module).to_string(),
                            );
                            let result = self.send_data_to_module(response, memory, caller).await;
                            return Ok(Some(result));
                        }
                    }
                }

                let runtime = caller.data().runtime.clone();
                self.handle_module_call(module_call, memory, caller, caller_module_schema, runtime)
                    .await
            }
        };
    }

    async fn send_bytes_to_module(
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

        let ptr = alloc
            .call_async(&mut caller, bytes.len() as i32)
            .await
            .unwrap();

        memory.write(&mut caller, ptr as usize, bytes).unwrap();

        (ptr, bytes.len() as i32)
    }

    pub async fn send_data_to_module<T>(
        &self,
        result: T,
        memory: &wasmtime::Memory,
        caller: &mut Caller<'_, StoreState>,
    ) -> i64
    where
        T: Serialize,
    {
        let encoded = encode(&result).unwrap();
        let (ptr, len) = self.send_bytes_to_module(memory, caller, &encoded).await;
        return pack_ptr_len(ptr, len);
    }
}
