use interstice_abi::{CallQueryRequest, IntersticeValue, ModuleSelection, NodeSelection};

use crate::{IntersticeError, NetworkPacket, runtime::Runtime};


/// How long a reducer or query waits for a remote query (DESIGN §7.1).
const REMOTE_QUERY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

impl Runtime {
    pub(crate) fn handle_call_query(
        &self,
        caller_module_name: &String,
        call_query_request: CallQueryRequest,
    ) -> Result<IntersticeValue, IntersticeError> {
        let module_name = match &call_query_request.module_selection {
            ModuleSelection::Current => caller_module_name,
            ModuleSelection::Other(name) => name,
        };
        match call_query_request.node_selection {
            NodeSelection::Current => {
                self.call_query(
                    module_name,
                    &call_query_request.query_name,
                    call_query_request.input,
                    self.network_handle.node_id,
                    caller_module_name,
                )
            }
            NodeSelection::Other(node_name) => {
                let network = self.network_handle.clone();
                let node_id = {
                    let modules = self.modules.lock();
                    let module = modules.get(caller_module_name).ok_or_else(|| {
                        IntersticeError::ModuleNotFound(
                            caller_module_name.clone(),
                            "Caller module missing while dispatching query".into(),
                        )
                    })?;
                    let node_dependency = module
                        .schema
                        .node_dependencies
                        .iter()
                        .find(|n| n.name == node_name)
                        .ok_or_else(|| {
                            IntersticeError::Internal(format!(
                                "Couldn't find node {node_name} in node dependencies"
                            ))
                        })?;
                    network
                        .get_node_id_from_adress(&node_dependency.address)
                        .map_err(|_| IntersticeError::UnknownPeer)?
                };
                let mut input = call_query_request.input;
                crate::runtime::blob::inline_blobs(&mut input).map_err(IntersticeError::Internal)?;
                let request_id = uuid::Uuid::new_v4().to_string();

                // Use a sync channel so the WASM thread can do a blocking recv.
                let (sender, receiver) = std::sync::mpsc::channel::<Result<IntersticeValue, String>>();
                self.pending_query_responses
                    .lock()
                    .insert(request_id.clone(), sender);

                network.send_packet(
                    node_id,
                    NetworkPacket::QueryCall {
                        request_id: request_id.clone(),
                        module_name: module_name.clone(),
                        query_name: call_query_request.query_name.clone(),
                        input,
                    },
                );

                // Block the WASM thread until the tokio event loop delivers
                // the response, or the timeout (DESIGN §7.1).
                let reply = receiver.recv_timeout(REMOTE_QUERY_TIMEOUT);
                self.pending_query_responses.lock().remove(&request_id);
                match reply {
                    Ok(result) => result.map_err(|e| IntersticeError::Internal(format!("remote query failed: {e}"))),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(IntersticeError::Internal(format!(
                        "remote query '{}' timed out after {:?}",
                        call_query_request.query_name, REMOTE_QUERY_TIMEOUT
                    ))),
                    Err(_) => Err(IntersticeError::ProtocolError("Query response channel closed".into())),
                }
            }
        }
    }
}
