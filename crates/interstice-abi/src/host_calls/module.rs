use crate::{NodeSelection, interstice_abi_macros::IntersticeType};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum ModuleCall {
    Load {
        node_selection: NodeSelection,
        wasm_binary: Vec<u8>,
    },
    /// Unload a module from the runtime while keeping its persisted table data,
    /// so it can be loaded again later and resume with its state intact.
    Unload {
        node_selection: NodeSelection,
        module_name: String,
    },
    /// Remove a module and delete all of its persisted data (full uninstall).
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
    LoadRequest {
        node_id: String,
        module_name: String,
        wasm_binary: Vec<u8>,
    },
    RemoveRequest {
        node_id: String,
        module_name: String,
    },
}
