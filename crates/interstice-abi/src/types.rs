use serde::{Deserialize, Serialize};

pub const ABI_VERSION: u16 = 1;

/// Opaque identifiers used across modules
pub type ModuleId = u64;
pub type ReducerId = u64;
pub type TableId = u64;
pub type SubscriptionId = u64;

/// Generic byte buffer for ABI serialization
pub type AbiBytes = Vec<u8>;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum PrimitiveType {
    I32,
    I64,
    F32,
    F64,
    Bool,
    String,
}
