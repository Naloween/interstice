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
    /// Event tables (`#[table(event = N)]`): keep only the `N` most recently
    /// inserted rows; older ones are evicted by the engine after each commit,
    /// without delete events. Always ephemeral.
    #[serde(default)]
    pub retention: Option<u64>,
    /// Logged tables: how long the log is kept as history, in ms
    /// (`#[table(history = "7d")]`); `None` keeps everything (DESIGN §6.7).
    #[serde(default)]
    pub history: Option<u64>,
    /// Column defaults (`#[default(expr)]`), used when a migration appends a
    /// column (DESIGN §3.3). `Option<T>` columns default to `None` without one.
    #[serde(default)]
    pub defaults: Vec<ColumnDefault>,
}

/// A column's default value, postcard-encoded `IntersticeValue` (bytes, so
/// the schema stays representable in TOML bindings).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ColumnDefault {
    pub column: String,
    pub value: Vec<u8>,
}

impl ColumnDefault {
    pub fn new(column: &str, value: &crate::IntersticeValue) -> Self {
        Self { column: column.to_string(), value: crate::encode(value).unwrap_or_default() }
    }

    pub fn value(&self) -> Option<crate::IntersticeValue> {
        crate::decode(&self.value).ok()
    }
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
    /// The value a new column `name` takes in existing rows: its declared
    /// default, or `None` for an `Option` column.
    pub fn column_default(&self, name: &str) -> Option<crate::IntersticeValue> {
        if let Some(d) = self.defaults.iter().find(|d| d.column == name) {
            return d.value();
        }
        let field = self.fields.iter().find(|f| f.name == name)?;
        match field.field_type {
            crate::IntersticeType::Option(_) => Some(crate::IntersticeValue::Option(None)),
            _ => None,
        }
    }

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
