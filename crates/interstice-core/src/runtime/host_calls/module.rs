use crate::{
    error::IntersticeError, runtime::Runtime, runtime::wasm::StoreState,
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
                        // An update when it is loaded already, with the
                        // safe policy: a plan that needs a decision is
                        // refused (DESIGN §7.4). The holder decided, so the
                        // authorities are granted.
                        use crate::network::protocol::LoadOutcome;
                        let (module_name, outcome) = Runtime::update_module_named(
                            runtime.clone(),
                            wasm_binary,
                            crate::MigrationPolicy::Safe,
                            true,
                        )
                        .await;
                        let result = match outcome {
                            LoadOutcome::Loaded { .. } | LoadOutcome::Forwarded => Ok(()),
                            LoadOutcome::NeedsDecision { plan } => {
                                Err(format!("Load of '{module_name}' refused: the migration needs a decision\n{plan}"))
                            }
                            LoadOutcome::NeedsGrant { .. } => Err(format!("Load of '{module_name}' needs a grant")),
                            LoadOutcome::Failed(err) if module_name.is_empty() => {
                                Err(format!("Load failed while compiling module bytes: {err}"))
                            }
                            LoadOutcome::Failed(err) => Err(format!("Load of '{module_name}' failed: {err}")),
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
                            crate::network::protocol::ModuleEventInstance::Load {
                                wasm_binary,
                                policy: Default::default(),
                                request_id: String::new(),
                                grant: false,
                            },
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
        };

        let packed = self.send_data_to_module(response, _memory, _caller);
        Ok(Some(packed))
    }
}
