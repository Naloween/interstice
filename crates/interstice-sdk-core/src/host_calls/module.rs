use crate::host_calls::host_call;
use interstice_abi::{HostCall, ModuleCall, NodeSelection};

pub fn publish(node_selection: NodeSelection, wasm_binary: Vec<u8>) {
    host_call(HostCall::Module(ModuleCall::Publish {
        node_selection,
        wasm_binary,
    }));
}

pub fn remove(node_selection: NodeSelection, module_name: String) {
    host_call(HostCall::Module(ModuleCall::Remove {
        node_selection,
        module_name,
    }));
}

pub struct ModuleAuthority;

impl ModuleAuthority {
    pub fn publish(&self, node_selection: NodeSelection, wasm_binary: Vec<u8>) {
        publish(node_selection, wasm_binary);
    }

    pub fn remove(&self, node_selection: NodeSelection, module_name: String) {
        remove(node_selection, module_name);
    }
}
