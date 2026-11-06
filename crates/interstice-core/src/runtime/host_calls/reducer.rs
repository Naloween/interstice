use crate::{
    error::IntersticeError,
    network::protocol::NetworkPacket,
    runtime::{Runtime, reducer::{CallFrameKind, OutboxEntry, PendingCall, CALL_STACK}},
};
use interstice_abi::{CallReducerRequest, ModuleSelection, NodeSelection};

impl Runtime {
    pub(crate) fn handle_call_reducer(
        &self,
        caller_module_name: &String,
        call_reducer_request: CallReducerRequest,
    ) -> Result<(), IntersticeError> {
        let in_query = CALL_STACK.with(|s| {
            s.borrow().last().map_or(false, |f| f.kind == CallFrameKind::Query)
        });
        if in_query {
            return Err(IntersticeError::Internal(
                "Reducers cannot be called from a query context".into(),
            ));
        }
        let module_name = match &call_reducer_request.module_selection {
            ModuleSelection::Current => caller_module_name,
            ModuleSelection::Other(name) => name,
        };
        // Calls are deferred messages (DESIGN §6.6): validate the target, then
        // record the call in the transaction's outbox. It is released only if
        // the caller commits.
        let entry = match call_reducer_request.node_selection {
            NodeSelection::Current => {
                let known = self.modules.lock().get(module_name).map(|m| {
                    m.reducer_names.contains(&call_reducer_request.reducer_name).then(|| m.schema.clone())
                });
                match known {
                    None => {
                        return Err(IntersticeError::ModuleNotFound(
                            module_name.clone(),
                            format!(
                                "When '{caller_module_name}' called reducer '{}'",
                                call_reducer_request.reducer_name
                            ),
                        ));
                    }
                    Some(None) => {
                        return Err(IntersticeError::ReducerNotFound {
                            module: module_name.clone(),
                            reducer: call_reducer_request.reducer_name,
                        });
                    }
                    // Refuse now rather than when the deferred call runs
                    // (it is checked again there, DESIGN §7.4).
                    Some(Some(schema)) => {
                        self.security.check_call(
                            &schema,
                            &call_reducer_request.reducer_name,
                            false,
                            self.node_id,
                            caller_module_name,
                        )?;
                        OutboxEntry::Local(PendingCall {
                            module_name: module_name.clone(),
                            reducer_name: call_reducer_request.reducer_name,
                            input: call_reducer_request.input,
                        })
                    }
                }
            }
            NodeSelection::Other(node_name) => {
                let address = {
                    let modules = self.modules.lock();
                    let module = modules.get(caller_module_name).ok_or_else(|| {
                        IntersticeError::ModuleNotFound(
                            caller_module_name.clone(),
                            "Caller module missing while dispatching reducer".into(),
                        )
                    })?;
                    module
                        .schema
                        .node_dependencies
                        .iter()
                        .find(|n| n.name == node_name)
                        .ok_or_else(|| {
                            IntersticeError::Internal(format!(
                                "Couldn't find node {node_name} in node dependencies"
                            ))
                        })?
                        .address
                        .clone()
                };
                let node_id = self
                    .network_handle
                    .clone()
                    .get_node_id_from_adress(&address)
                    .map_err(|_| IntersticeError::UnknownPeer)?;
                // Inline now: the caller's blobs are gone by the time it is sent.
                let mut input = call_reducer_request.input;
                crate::runtime::blob::inline_blobs(&mut input).map_err(IntersticeError::Internal)?;
                OutboxEntry::Remote {
                    node_id,
                    packet: NetworkPacket::ReducerCall {
                        request_id: None,
                        module_name: module_name.clone(),
                        reducer_name: call_reducer_request.reducer_name,
                        input,
                    },
                }
            }
        };
        CALL_STACK.with(|s| {
            let mut stack = s.borrow_mut();
            let frame = stack
                .last_mut()
                .ok_or_else(|| IntersticeError::Internal("No active call frame".into()))?;
            frame.outbox.push(entry);
            Ok(())
        })
    }
}
