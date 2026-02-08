use interstice_abi::{CallQueryRequest, CallQueryResponse, ModuleSelection, NodeSelection};

use crate::{IntersticeError, NetworkPacket, runtime::Runtime};
use tokio::sync::oneshot;

impl Runtime {
    pub(crate) async fn handle_call_query(
        &self,
        caller_module_name: &String,
        call_query_request: CallQueryRequest,
    ) -> Result<CallQueryResponse, IntersticeError> {
        let module_name = match &call_query_request.module_selection {
            ModuleSelection::Current => caller_module_name,
            ModuleSelection::Other(name) => name,
        };
        match call_query_request.node_selection {
            NodeSelection::Current => {
                let result = self.call_query(
                    module_name,
                    &call_query_request.query_name,
                    call_query_request.input,
                ).await?;
                Ok(result)
            }
            NodeSelection::Other(node_name) => {
                let network = self.network_handle.clone();
                let node_id = {
                    let modules = self.modules.lock().unwrap();
                    let module = modules.get(caller_module_name).unwrap();
                    let node_dependency = module
                        .schema
                        .node_dependencies
                        .iter()
                        .find(|n| n.name == node_name)
                        .unwrap();
                    network
                        .get_node_id_from_adress(&node_dependency.address)
                        .unwrap()
                };
                let request_id = uuid::Uuid::new_v4().to_string();

                let (sender, receiver) = oneshot::channel();
                self.pending_query_responses
                    .lock()
                    .unwrap()
                    .insert(request_id.clone(), sender);

                network.send_packet(
                    node_id,
                    NetworkPacket::QueryCall {
                        request_id: request_id.clone(),
                        module_name: module_name.clone(),
                        query_name: call_query_request.query_name.clone(),
                        input: call_query_request.input.clone(),
                    },
                );
                let result = receiver
                    .await
                    .map_err(|_| IntersticeError::ProtocolError("Query response channel closed".into()))?;
                Ok(result)
            }
        }
    }
}
