use crate::host_calls::{host_call, unpack};
use interstice_abi::{HostCall, ModuleCall, ModuleCallResponse, NodeSelection};

pub fn load(node_selection: NodeSelection, wasm_binary: Vec<u8>) -> Result<(), String> {
    let pack = host_call(HostCall::Module(ModuleCall::Load {
        node_selection,
        wasm_binary,
    }));
    let response: ModuleCallResponse = unpack(pack);
    match response {
        ModuleCallResponse::Ok => Ok(()),
        ModuleCallResponse::Err(err) => Err(err),
    }
}

/// Unload a module while keeping its persisted table data intact.
pub fn unload(node_selection: NodeSelection, module_name: String) -> Result<(), String> {
    let pack = host_call(HostCall::Module(ModuleCall::Unload {
        node_selection,
        module_name,
    }));
    let response: ModuleCallResponse = unpack(pack);
    match response {
        ModuleCallResponse::Ok => Ok(()),
        ModuleCallResponse::Err(err) => Err(err),
    }
}

/// Remove a module and delete all of its persisted data (full uninstall).
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
    pub fn load(
        &self,
        node_selection: NodeSelection,
        wasm_binary: Vec<u8>,
    ) -> Result<(), String> {
        load(node_selection, wasm_binary)
    }

    pub fn unload(&self, node_selection: NodeSelection, module_name: String) -> Result<(), String> {
        unload(node_selection, module_name)
    }

    pub fn remove(&self, node_selection: NodeSelection, module_name: String) -> Result<(), String> {
        remove(node_selection, module_name)
    }
}
