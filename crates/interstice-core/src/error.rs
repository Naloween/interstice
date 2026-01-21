use std::fmt;

#[derive(Debug)]
pub enum IntersticeError {
    // ─── Module / Reducer resolution ──────────────────────────────────────
    ModuleAlreadyExists(String),
    ModuleNotFound(String),

    ReducerNotFound { module: String, reducer: String },

    ReducerCycle { module: String, reducer: String },

    // ─── WASM loading / linking ────────────────────────────────────────────
    MissingExport(&'static str),
    WasmFuncNotFound(String),
    BadSignature(String),
    InvalidSchema,
    AbiVersionMismatch { expected: u16, found: u16 },

    // ─── WASM execution ────────────────────────────────────────────────────
    WasmTrap(String),

    // ─── Memory handling ───────────────────────────────────────────────────
    MemoryRead,
    MemoryWrite,

    // ─── Internal invariants ───────────────────────────────────────────────
    Internal(&'static str),
}

impl fmt::Display for IntersticeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use IntersticeError::*;

        match self {
            ModuleAlreadyExists(name) => {
                write!(f, "module '{}' already exists", name)
            }
            ModuleNotFound(name) => {
                write!(f, "module '{}' not found", name)
            }
            ReducerNotFound { module, reducer } => {
                write!(f, "reducer '{}' not found in module '{}'", reducer, module)
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
