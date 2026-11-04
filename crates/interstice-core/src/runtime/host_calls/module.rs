use crate::{
    error::IntersticeError, runtime::Runtime, runtime::module::Module, runtime::wasm::StoreState,
};
use interstice_abi::{ModuleCall, ModuleCallResponse, ModuleSchema, NodeSelection};
use std::sync::Arc;
use wasmtime::{Caller, Memory};

impl Runtime {
    pub fn handle_module_call(
        &self,
        call: ModuleCall,
        _memory: &Memory,
        _caller: &mut Caller<'_, StoreState>,
        caller_module_schema: Arc<ModuleSchema>,
        runtime: Arc<Runtime>,
    ) -> Result<Option<i64>, IntersticeError> {
        let response = match call {
            ModuleCall::Load {
                node_selection,
                wasm_binary,
            } => match node_selection {
                NodeSelection::Current => {
                    // Spawn on the tokio runtime (not spawn_local — we're on a std::thread).
                    // The outcome goes back to the requester as a
                    // `module_load_result` event (DESIGN §9).
                    let requester = caller_module_schema.name.clone();
                    self.tokio_handle.spawn(async move {
                        let (module_name, result) =
                            match Module::from_bytes(runtime.clone(), &wasm_binary).await {
                                Ok(module) => {
                                    let name = module.schema.name.clone();
                                    let result = Runtime::load_module(runtime.clone(), module)
                                        .await
                                        .map(|_| ())
                                        .map_err(|err| format!("Load of '{name}' failed: {err}"));
                                    (name, result)
                                }
                                Err(err) => (
                                    String::new(),
                                    Err(format!("Load failed while compiling module bytes: {err}")),
                                ),
                            };
                        if let Err(message) = &result {
                            runtime.logger.log(
                                message,
                                crate::logger::LogSource::Runtime,
                                crate::logger::LogLevel::Error,
                            );
                        }
                        let (ok, message) = match result {
                            Ok(()) => (true, format!("'{module_name}' loaded")),
                            Err(message) => (false, message),
                        };
                        let _ = runtime.event_sender.send((
                            crate::runtime::event::EventInstance::Module(
                                interstice_abi::ModuleEvent::LoadResult {
                                    requester,
                                    module_name,
                                    ok,
                                    message,
                                },
                            ),
                            None,
                        ));
                    });
                    ModuleCallResponse::Ok
                }
                NodeSelection::Other(node_name) => {
                    let node_dependency = caller_module_schema
                        .node_dependencies
                        .iter()
                        .find(|n| n.name == node_name);
                    let node_address = match node_dependency {
                        Some(dep) => dep.address.clone(),
                        None => {
                            return Ok(Some(
                                self.send_data_to_module(
                                    ModuleCallResponse::Err(format!(
                                        "Couldn't find node {node_name} in node dependencies"
                                    )),
                                    _memory,
                                    _caller,
                                ),
                            ));
                        }
                    };
                    let node_id = match self.network_handle.get_node_id_from_adress(&node_address) {
                        Ok(node_id) => node_id,
                        Err(err) => {
                            return Ok(Some(
                                self.send_data_to_module(
                                    ModuleCallResponse::Err(err.to_string()),
                                    _memory,
                                    _caller,
                                ),
                            ));
                        }
                    };
                    self.network_handle.send_packet(
                        node_id,
                        crate::network::protocol::NetworkPacket::ModuleEvent(
                            crate::network::protocol::ModuleEventInstance::Load { wasm_binary },
                        ),
                    );
                    ModuleCallResponse::Ok
                }
            },
            ModuleCall::Unload {
                node_selection,
                module_name,
            } => match node_selection {
                NodeSelection::Current => {
                    Runtime::unload_module(runtime.clone(), &module_name);
                    ModuleCallResponse::Ok
                }
                NodeSelection::Other(node_name) => {
                    let node_dependency = caller_module_schema
                        .node_dependencies
                        .iter()
                        .find(|n| n.name == node_name);
                    let node_address = match node_dependency {
                        Some(dep) => dep.address.clone(),
                        None => {
                            return Ok(Some(
                                self.send_data_to_module(
                                    ModuleCallResponse::Err(format!(
                                        "Couldn't find node {node_name} in node dependencies"
                                    )),
                                    _memory,
                                    _caller,
                                ),
                            ));
                        }
                    };
                    let node_id = match self.network_handle.get_node_id_from_adress(&node_address) {
                        Ok(node_id) => node_id,
                        Err(err) => {
                            return Ok(Some(
                                self.send_data_to_module(
                                    ModuleCallResponse::Err(err.to_string()),
                                    _memory,
                                    _caller,
                                ),
                            ));
                        }
                    };
                    self.network_handle.send_packet(
                        node_id,
                        crate::network::protocol::NetworkPacket::ModuleEvent(
                            crate::network::protocol::ModuleEventInstance::Unload { module_name },
                        ),
                    );
                    ModuleCallResponse::Ok
                }
            },
            ModuleCall::Remove {
                node_selection,
                module_name,
            } => match node_selection {
                NodeSelection::Current => {
                    Runtime::remove_module(runtime.clone(), &module_name);
                    ModuleCallResponse::Ok
                }
                NodeSelection::Other(node_name) => {
                    let node_dependency = caller_module_schema
                        .node_dependencies
                        .iter()
                        .find(|n| n.name == node_name);
                    let node_address = match node_dependency {
                        Some(dep) => dep.address.clone(),
                        None => {
                            return Ok(Some(
                                self.send_data_to_module(
                                    ModuleCallResponse::Err(format!(
                                        "Couldn't find node {node_name} in node dependencies"
                                    )),
                                    _memory,
                                    _caller,
                                ),
                            ));
                        }
                    };
                    let node_id = match self.network_handle.get_node_id_from_adress(&node_address) {
                        Ok(node_id) => node_id,
                        Err(err) => {
                            return Ok(Some(
                                self.send_data_to_module(
                                    ModuleCallResponse::Err(err.to_string()),
                                    _memory,
                                    _caller,
                                ),
                            ));
                        }
                    };
                    self.network_handle.send_packet(
                        node_id,
                        crate::network::protocol::NetworkPacket::ModuleEvent(
                            crate::network::protocol::ModuleEventInstance::Remove { module_name },
                        ),
                    );
                    ModuleCallResponse::Ok
                }
            },
            ModuleCall::Record { module_name, enabled } => {
                let module = self.modules.lock().get(&module_name).cloned();
                match module {
                    None => ModuleCallResponse::Err(format!("module '{module_name}' is not loaded")),
                    Some(module) => {
                        if enabled {
                            module.start_recording(crate::runtime::record::DEFAULT_WINDOW);
                        } else {
                            module.stop_recording();
                        }
                        let state = if enabled { "started" } else { "stopped" };
                        self.log_record_event(&format!("record: '{module_name}' {state}"), false);
                        ModuleCallResponse::Ok
                    }
                }
            }
            ModuleCall::Replay { module_name } => match runtime.replay_module(&module_name) {
                Ok(report) => {
                    self.log_record_event(&format!("replay: {report}"), false);
                    ModuleCallResponse::Report(report)
                }
                Err(divergence) => {
                    let message = format!("replay of '{module_name}' diverged: {divergence}");
                    self.log_record_event(&message, true);
                    ModuleCallResponse::Err(message)
                }
            },
        };

        let packed = self.send_data_to_module(response, _memory, _caller);
        Ok(Some(packed))
    }
}
