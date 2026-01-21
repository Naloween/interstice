use crate::types::PrimitiveType;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReducerSchema {
    pub name: String,
    pub arguments: Vec<PrimitiveType>, // simple type names for now
    pub return_type: Option<PrimitiveType>,
}

impl ReducerSchema {
    pub fn new(
        name: impl Into<String>,
        arguments: Vec<PrimitiveType>,
        return_type: Option<PrimitiveType>,
    ) -> Self {
        Self {
            name: name.into(),
            arguments,
            return_type,
        }
    }
}
