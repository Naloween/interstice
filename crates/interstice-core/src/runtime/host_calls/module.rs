use crate::{
    error::IntersticeError, runtime::Runtime, runtime::module::Module, runtime::wasm::StoreState,
};
use interstice_abi::{ModuleCall, ModuleSchema, NodeSelection};
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
        match call {
            ModuleCall::Publish {
                node_selection,
                wasm_binary,
            } => match node_selection {
                NodeSelection::Current => {
                    tokio::task::spawn_local(async move {
                        let _ = Runtime::load_module(
                            runtime.clone(),
                            Module::from_bytes(runtime.clone(), &wasm_binary)
                                .await
                                .unwrap(),
                            true,
                        )
                        .await;
                    });
                }
                NodeSelection::Other(node_name) => {
                    let node_address = caller_module_schema
                        .node_dependencies
                        .iter()
                        .find(|n| n.name == node_name)
                        .ok_or_else(|| {
                            IntersticeError::Internal(format!(
                                "Couldn't find node {node_name} in node dependencies"
                            ))
                        })?
                        .address
                        .clone();
                    let node_id = self.network_handle.get_node_id_from_adress(&node_address)?;
                    self.network_handle.send_packet(
                        node_id,
                        crate::network::protocol::NetworkPacket::ModuleEvent(
                            crate::network::protocol::ModuleEventInstance::Publish { wasm_binary },
                        ),
                    );
                }
            },
            ModuleCall::Remove {
                node_selection,
                module_name,
            } => match node_selection {
                NodeSelection::Current => {
                    Runtime::remove_module(runtime.clone(), &module_name);
                }
                NodeSelection::Other(node_name) => {
                    let node_address = caller_module_schema
                        .node_dependencies
                        .iter()
                        .find(|n| n.name == node_name)
                        .ok_or_else(|| {
                            IntersticeError::Internal(format!(
                                "Couldn't find node {node_name} in node dependencies"
                            ))
                        })?
                        .address
                        .clone();
                    let node_id = self.network_handle.get_node_id_from_adress(&node_address)?;
                    self.network_handle.send_packet(
                        node_id,
                        crate::network::protocol::NetworkPacket::ModuleEvent(
                            crate::network::protocol::ModuleEventInstance::Remove { module_name },
                        ),
                    );
                }
            },
        }

        Ok(None)
    }
}
