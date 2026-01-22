use crate::{ABI_VERSION, PrimitiveType};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EntrySchema {
    pub name: String,
    pub value_type: PrimitiveType,
}

pub type Entries = Vec<EntrySchema>;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModuleSchema {
    pub abi_version: u16,
    pub name: String,
    pub version: u32,
    pub reducers: Vec<ReducerSchema>,
    pub tables: Vec<TableSchema>,
}

impl ModuleSchema {
    pub fn new(
        name: impl Into<String>,
        version: u32,
        reducers: Vec<ReducerSchema>,
        tables: Vec<TableSchema>,
    ) -> Self {
        Self {
            abi_version: ABI_VERSION,
            name: name.into(),
            version,
            reducers,
            tables,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReducerSchema {
    pub name: String,
    pub arguments: Entries,
    pub return_type: PrimitiveType,
}

impl ReducerSchema {
    pub fn new(name: impl Into<String>, arguments: Entries, return_type: PrimitiveType) -> Self {
        Self {
            name: name.into(),
            arguments,
            return_type,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TableSchema {
    pub name: String,
    pub visibility: TableVisibility,
    pub row_type: Entries,
    pub primary_key: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum TableVisibility {
    Public,
    Private,
}
