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
    QueryNotFound {
        module: String,
        query: String,
    },
    /// A reducer or query returned `Err`, or panicked, with this message.
    CallFailed(String),
    RowNotFound {
        primary_key_value: interstice_abi::IndexKey,
    },
    UniqueConstraintViolation {
        table_name: String,
        field_name: String,
    },
    IndexNotFound {
        table_name: String,
        field_name: String,
    },
    IndexQueryUnsupported {
        table_name: String,
        field_name: String,
    },
    AutoIncUpdateNotAllowed {
        table_name: String,
        field_name: String,
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
    /// A load whose migration plan needs a decision (DESIGN §3.3); the
    /// string is the rendered plan.
    MigrationRequired(String),
    /// A call or request the caller may not make (DESIGN §7.4).
    PermissionDenied(String),
    AbiVersionMismatch {
        expected: u16,
        found: u16,
    },

    // ─── WASM execution ────────────────────────────────────────────────────
    WasmTrap(String),

    // ─── Memory handling ───────────────────────────────────────────────────
    MemoryRead,
    MemoryWrite,

    // Network
    NetworkSendFailed,
    UnknownPeer,
    ProtocolError(String),

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
            RowNotFound { primary_key_value } => {
                write!(
                    f,
                    "row with primary key value {:?} not found",
                    primary_key_value
                )
            }
            UniqueConstraintViolation {
                table_name,
                field_name,
            } => {
                write!(
                    f,
                    "unique constraint violation on table '{}' field '{}'",
                    table_name, field_name
                )
            }
            IndexNotFound {
                table_name,
                field_name,
            } => {
                write!(
                    f,
                    "index not found on table '{}' field '{}'",
                    table_name, field_name
                )
            }
            IndexQueryUnsupported {
                table_name,
                field_name,
            } => {
                write!(
                    f,
                    "index query not supported for table '{}' field '{}'",
                    table_name, field_name
                )
            }
            AutoIncUpdateNotAllowed {
                table_name,
                field_name,
            } => {
                write!(
                    f,
                    "auto_inc field '{}' on table '{}' cannot be updated",
                    field_name, table_name
                )
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
            CallFailed(message) => write!(f, "{message}"),
            QueryNotFound { module, query } => {
                write!(f, "query '{}' not found in module '{}'", query, module)
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
            MigrationRequired(plan) => write!(f, "{}", plan),
            PermissionDenied(why) => write!(f, "Permission denied: {}", why),
            AbiVersionMismatch { expected, found } => {
                write!(
                    f,
                    "module built for ABI {found}; this interstice loads ABI {}..={expected}, rebuild it",
                    interstice_abi::MIN_ABI_VERSION
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
            NetworkSendFailed => {
                write!(f, "failed to send packet")
            }
            UnknownPeer => {
                write!(f, "failed to find peer")
            }
            ProtocolError(msg) => {
                write!(f, "Network protocol error: {msg}")
            }
            Internal(msg) => {
                write!(f, "internal error: {}", msg)
            }
        }
    }
}

impl std::error::Error for IntersticeError {}
