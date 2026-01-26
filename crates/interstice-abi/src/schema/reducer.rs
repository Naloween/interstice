use serde::{Deserialize, Serialize};

use crate::{IntersticeType, entry::Entries, event::TableEvent};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReducerSchema {
    pub name: String,
    pub arguments: Entries,
    pub return_type: IntersticeType,
}

impl ReducerSchema {
    pub fn new(name: impl Into<String>, arguments: Entries, return_type: IntersticeType) -> Self {
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
