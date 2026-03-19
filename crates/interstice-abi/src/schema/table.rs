use std::collections::HashMap;

use crate::{IntersticeTypeDef, Row, interstice_type_def::FieldDef, validate_value_detailed};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TableSchema {
    pub name: String,
    pub type_name: String,
    pub visibility: TableVisibility,
    pub fields: Vec<FieldDef>,
    pub primary_key: FieldDef,
    pub primary_key_auto_inc: bool,
    pub indexes: Vec<IndexSchema>,
    pub persistence: PersistenceKind,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub enum TableVisibility {
    Public,
    Private,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub enum PersistenceKind {
    Logged,
    Stateful,
    Ephemeral,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub enum IndexType {
    Hash,
    BTree,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IndexSchema {
    pub field_name: String,
    pub index_type: IndexType,
    pub unique: bool,
    pub auto_inc: bool,
}

impl TableSchema {
    pub fn validate_row(
        &self,
        row: &Row,
        type_definitions: &HashMap<String, IntersticeTypeDef>,
    ) -> Result<(), String> {
        // For auto-inc primary keys the host assigns the value, so the WASM sends a
        // placeholder (Void). Skip primary key validation in that case.
        if !self.primary_key_auto_inc {
            validate_value_detailed(&row.primary_key, &self.primary_key.field_type, type_definitions)
                .map_err(|e| format!("primary key '{}': {}", self.primary_key.name, e))?;
        }
        if row.entries.len() != self.fields.len() {
            return Err(format!(
                "field count mismatch: expected {}, got {}",
                self.fields.len(),
                row.entries.len()
            ));
        }
        for (entry, ty) in row.entries.iter().zip(self.fields.iter()) {
            validate_value_detailed(entry, &ty.field_type, type_definitions)
                .map_err(|e| format!("field '{}': {}", ty.name, e))?;
        }
        Ok(())
    }
}
