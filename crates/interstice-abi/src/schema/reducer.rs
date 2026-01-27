use serde::{Deserialize, Serialize};

use crate::{IntersticeType, event::TableEvent, interstice_type_def::FieldDef};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReducerSchema {
    pub name: String,
    pub arguments: Vec<FieldDef>,
    pub return_type: IntersticeType,
}

impl ReducerSchema {
    pub fn new(
        name: impl Into<String>,
        arguments: Vec<FieldDef>,
        return_type: IntersticeType,
    ) -> Self {
        Self {
            name: name.into(),
            arguments,
            return_type,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SubscriptionSchema {
    pub module_name: String,
    pub table_name: String,
    pub reducer_name: String,
    pub event: TableEvent,
}
