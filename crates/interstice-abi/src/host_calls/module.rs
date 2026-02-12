use crate::{NodeSelection, interstice_abi_macros::IntersticeType};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum ModuleCall {
    Publish {
        node_selection: NodeSelection,
        wasm_binary: Vec<u8>,
    },
    Remove {
        node_selection: NodeSelection,
        module_name: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ModuleCallResponse {
    Ok,
    Err(String),
}

#[derive(Debug, Deserialize, Serialize, IntersticeType, Clone)]
pub enum ModuleEvent {
    PublishRequest {
        node_id: String,
        module_name: String,
        wasm_binary: Vec<u8>,
    },
    RemoveRequest {
        node_id: String,
        module_name: String,
    },
}
