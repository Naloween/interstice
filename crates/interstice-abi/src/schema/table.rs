use std::collections::HashMap;

use crate::{IntersticeTypeDef, Row, interstice_type_def::FieldDef, validate_value};
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
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub enum TableVisibility {
    Public,
    Private,
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
    ) -> bool {
        if !validate_value(
            &row.primary_key,
            &self.primary_key.field_type,
            type_definitions,
        ) {
            return false;
        }
        if row.entries.len() != self.fields.len() {
            return false;
        }
        for (entry, ty) in row.entries.iter().zip(self.fields.iter()) {
            if !validate_value(entry, &ty.field_type, type_definitions) {
                return false;
            }
        }
        true
    }
}
