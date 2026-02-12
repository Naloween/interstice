use crate::host_calls::{host_call, unpack};
use interstice_abi::{HostCall, ModuleCall, ModuleCallResponse, NodeSelection};

pub fn publish(node_selection: NodeSelection, wasm_binary: Vec<u8>) -> Result<(), String> {
    let pack = host_call(HostCall::Module(ModuleCall::Publish {
        node_selection,
        wasm_binary,
    }));
    let response: ModuleCallResponse = unpack(pack);
    match response {
        ModuleCallResponse::Ok => Ok(()),
        ModuleCallResponse::Err(err) => Err(err),
    }
}

pub fn remove(node_selection: NodeSelection, module_name: String) -> Result<(), String> {
    let pack = host_call(HostCall::Module(ModuleCall::Remove {
        node_selection,
        module_name,
    }));
    let response: ModuleCallResponse = unpack(pack);
    match response {
        ModuleCallResponse::Ok => Ok(()),
        ModuleCallResponse::Err(err) => Err(err),
    }
}

pub struct ModuleAuthority;

impl ModuleAuthority {
    pub fn publish(
        &self,
        node_selection: NodeSelection,
        wasm_binary: Vec<u8>,
    ) -> Result<(), String> {
        publish(node_selection, wasm_binary)
    }

    pub fn remove(&self, node_selection: NodeSelection, module_name: String) -> Result<(), String> {
        remove(node_selection, module_name)
    }
}
