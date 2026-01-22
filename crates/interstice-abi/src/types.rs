use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Row {
    pub primary_key: PrimitiveValue,
    pub entries: Vec<PrimitiveValue>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum PrimitiveType {
    Void,
    I32,
    I64,
    F32,
    F64,
    Bool,
    String,
    Vec(Box<PrimitiveType>),
    Option(Box<PrimitiveType>),
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub enum PrimitiveValue {
    Void,
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    Bool(bool),
    String(String),
    Vec(Vec<PrimitiveValue>),
    Option(Option<Box<PrimitiveValue>>),
}

pub fn validate_value(value: &PrimitiveValue, ty: &PrimitiveType) -> bool {
    match (value, ty) {
        (PrimitiveValue::Void, PrimitiveType::Void) => true,
        (PrimitiveValue::I64(_), PrimitiveType::I64) => true,
        (PrimitiveValue::F64(_), PrimitiveType::F64) => true,
        (PrimitiveValue::F32(_), PrimitiveType::F32) => true,
        (PrimitiveValue::Bool(_), PrimitiveType::Bool) => true,
        (PrimitiveValue::I32(_), PrimitiveType::I32) => true,
        (PrimitiveValue::String(_), PrimitiveType::String) => true,
        (PrimitiveValue::Vec(v), PrimitiveType::Vec(inner)) => {
            v.iter().all(|x| validate_value(x, inner))
        }
        (PrimitiveValue::Option(None), PrimitiveType::Option(_)) => true,
        (PrimitiveValue::Option(Some(v)), PrimitiveType::Option(inner)) => validate_value(v, inner),
        _ => false,
    }
}
