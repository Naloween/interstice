use crate::{ABI_VERSION, PrimitiveType};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModuleSchema {
    pub abi_version: u16,
    pub name: String,
    pub version: u32,
    pub reducers: Vec<ReducerSchema>,
    // tables will be added later
}

impl ModuleSchema {
    pub fn new(name: impl Into<String>, version: u32, reducers: Vec<ReducerSchema>) -> Self {
        Self {
            abi_version: ABI_VERSION,
            name: name.into(),
            version,
            reducers,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReducerSchema {
    pub name: String,
    pub arguments: PrimitiveType, // simple type names for now
    pub return_type: Option<PrimitiveType>,
}

impl ReducerSchema {
    pub fn new(
        name: impl Into<String>,
        arguments: PrimitiveType,
        return_type: Option<PrimitiveType>,
    ) -> Self {
        Self {
            name: name.into(),
            arguments,
            return_type,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TableSchema {
    pub name: String,
    pub visibility: TableVisibility,
    // schema details will come later
}

#[derive(Debug, Clone)]
pub enum TableVisibility {
    Public,
    Private,
}
