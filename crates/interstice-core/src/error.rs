use std::fmt;

use interstice_abi::{Authority, Version};

use crate::node::NodeId;

#[derive(Debug)]
pub enum IntersticeError {
    // Node
    NodeNotFound(NodeId),
    // Authority
    AuthorityAlreadyTaken(String, String, String),
    Unauthorized(Authority),
    // ─── Module / Reducer resolution ──────────────────────────────────────
    ModuleAlreadyExists(String),
    ModuleNotFound(String, String),
    ModuleVersionMismatch(String, String, Version, Version),

    TableNotFound {
        module_name: String,
        table_name: String,
    },
    ReducerNotFound {
        module: String,
        reducer: String,
    },
    InvalidRow {
        module: String,
        table: String,
    },
    ReducerCycle {
        module: String,
        reducer: String,
    },

    // ─── WASM loading / linking ────────────────────────────────────────────
    MissingExport(&'static str),
    WasmFuncNotFound(String),
    BadSignature(String),
    InvalidSchema,
    AbiVersionMismatch {
        expected: u16,
        found: u16,
    },

    // ─── WASM execution ────────────────────────────────────────────────────
    WasmTrap(String),

    // ─── Memory handling ───────────────────────────────────────────────────
    MemoryRead,
    MemoryWrite,

    // ─── Internal invariants ───────────────────────────────────────────────
    Internal(String),
}

impl fmt::Display for IntersticeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use IntersticeError::*;

        match self {
            NodeNotFound(node_id) => {
                write!(f, "Node '{}' not found", node_id)
            }
            ModuleVersionMismatch(
                module_name,
                dependency_module_name,
                req_version,
                actual_version,
            ) => {
                write!(
                    f,
                    "module '{}' requires module {} with version {} but is {}",
                    module_name,
                    dependency_module_name,
                    Into::<String>::into(req_version.clone()),
                    Into::<String>::into(actual_version.clone())
                )
            }
            ModuleAlreadyExists(name) => {
                write!(f, "module '{}' already exists", name)
            }
            AuthorityAlreadyTaken(name, authority, in_place_module_name) => {
                write!(
                    f,
                    "module '{}' require already taken authority {} by module {}",
                    name, authority, in_place_module_name
                )
            }
            Unauthorized(authority) => {
                write!(f, "module does not have {:?} authority", authority)
            }
            ModuleNotFound(name, context) => {
                write!(f, "module '{}' not found. {}", name, context)
            }
            TableNotFound {
                module_name: module,
                table_name: table,
            } => {
                write!(f, "table '{}' not found in module '{}'", table, module)
            }
            ReducerNotFound { module, reducer } => {
                write!(f, "reducer '{}' not found in module '{}'", reducer, module)
            }
            InvalidRow { module, table } => {
                write!(
                    f,
                    "invalid row encountered on transaction in module {} for table '{}'",
                    module, table
                )
            }
            ReducerCycle { module, reducer } => {
                write!(
                    f,
                    "reducer cycle detected while calling '{}::{}'",
                    module, reducer
                )
            }
            MissingExport(name) => {
                write!(f, "missing required wasm export '{}'", name)
            }
            WasmFuncNotFound(name) => {
                write!(f, "wasm function '{}' not found", name)
            }
            BadSignature(name) => {
                write!(f, "invalid wasm signature for '{}'", name)
            }
            InvalidSchema => {
                write!(f, "invalid module schema")
            }
            AbiVersionMismatch { expected, found } => {
                write!(
                    f,
                    "ABI version mismatch: expected {}, found {}",
                    expected, found
                )
            }
            WasmTrap(msg) => {
                write!(f, "wasm trapped: {}", msg)
            }
            MemoryRead => {
                write!(f, "failed to read from wasm memory")
            }
            MemoryWrite => {
                write!(f, "failed to write to wasm memory")
            }
            Internal(msg) => {
                write!(f, "internal error: {}", msg)
            }
        }
    }
}

impl std::error::Error for IntersticeError {}
