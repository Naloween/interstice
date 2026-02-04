use crate::{Node, error::IntersticeError, network::protocol::NetworkPacket};
use interstice_abi::{
    CallReducerRequest, CallReducerResponse, IntersticeValue, ModuleSelection, NodeSelection,
};

impl Node {
    pub(crate) fn handle_call_reducer(
        &mut self,
        caller_module_name: &String,
        call_reducer_request: CallReducerRequest,
    ) -> Result<CallReducerResponse, IntersticeError> {
        let module_name = match &call_reducer_request.module_selection {
            ModuleSelection::Current => caller_module_name,
            ModuleSelection::Other(name) => name,
        };
        match call_reducer_request.node_selection {
            NodeSelection::Current => {
                let (result, events) = self.invoke_reducer(
                    module_name,
                    &call_reducer_request.reducer_name,
                    call_reducer_request.input,
                )?;

                let frame = self.call_stack.last_mut().unwrap();
                frame.emitted_events.extend(events);
                Ok(result)
            }
            NodeSelection::Other(node_name) => {
                let module = self.modules.get(module_name).unwrap();
                let node_dependency = module
                    .schema
                    .node_dependencies
                    .iter()
                    .find(|n| n.name == node_name)
                    .unwrap();
                let network = &mut self.network_handle;
                let node_id = network
                    .get_node_id_from_adress(&node_dependency.address)
                    .unwrap();
                network.send_packet(
                    node_id,
                    NetworkPacket::ReducerCall {
                        module_name: module_name.clone(),
                        reducer_name: call_reducer_request.reducer_name.clone(),
                    },
                );
                Ok(IntersticeValue::Void)
            }
        }
    }
}
