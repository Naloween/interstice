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
        caller_module_schema: ModuleSchema,
        runtime: Arc<Runtime>,
    ) -> Result<Option<i64>, IntersticeError> {
        let response = match call {
            ModuleCall::Publish {
                node_selection,
                wasm_binary,
            } => match node_selection {
                NodeSelection::Current => {
                    // Spawn on the tokio runtime (not spawn_local — we're on a std::thread).
                    self.tokio_handle.spawn(async move {
                        match Module::from_bytes(runtime.clone(), &wasm_binary).await {
                            Ok(module) => {
                                if let Err(err) =
                                    Runtime::load_module(runtime.clone(), module, true).await
                                {
                                    runtime.logger.log(
                                        &format!(
                                            "Local publish failed while loading module: {}",
                                            err
                                        ),
                                        crate::logger::LogSource::Runtime,
                                        crate::logger::LogLevel::Error,
                                    );
                                }
                            }
                            Err(err) => {
                                runtime.logger.log(
                                    &format!(
                                        "Local publish failed while instantiating module bytes: {}",
                                        err
                                    ),
                                    crate::logger::LogSource::Runtime,
                                    crate::logger::LogLevel::Error,
                                );
                            }
                        }
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
                            crate::network::protocol::ModuleEventInstance::Publish { wasm_binary },
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
