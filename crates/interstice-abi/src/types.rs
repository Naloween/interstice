use serde::{Deserialize, Serialize};

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
    Vec(Box<PrimitiveType>),
}

impl PrimitiveType {
    pub fn matches(&self, value: &PrimitiveValue) -> bool {
        matches!(
            (self, value),
            (PrimitiveType::I32, PrimitiveValue::I32(_))
                | (PrimitiveType::I64, PrimitiveValue::I64(_))
                | (PrimitiveType::F32, PrimitiveValue::F32(_))
                | (PrimitiveType::F64, PrimitiveValue::F64(_))
                | (PrimitiveType::Bool, PrimitiveValue::Bool(_))
                | (PrimitiveType::String, PrimitiveValue::String(_))
                | (PrimitiveType::Vec(_), PrimitiveValue::Vec(_))
        )
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum PrimitiveValue {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    Bool(bool),
    String(String),
    Vec(Vec<PrimitiveValue>),
}
