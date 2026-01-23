use crate::{ABI_VERSION, IntersticeType};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EntrySchema {
    pub name: String,
    pub value_type: IntersticeType,
}

pub type Entries = Vec<EntrySchema>;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModuleSchema {
    pub abi_version: u16,
    pub name: String,
    pub version: Version,
    pub reducers: Vec<ReducerSchema>,
    pub tables: Vec<TableSchema>,
    pub subscriptions: Vec<SubscriptionSchema>,
}

impl ModuleSchema {
    pub fn new(
        name: impl Into<String>,
        version: Version,
        reducers: Vec<ReducerSchema>,
        tables: Vec<TableSchema>,
        subscriptions: Vec<SubscriptionSchema>,
    ) -> Self {
        Self {
            abi_version: ABI_VERSION,
            name: name.into(),
            version,
            reducers,
            tables,
            subscriptions,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl Into<String> for Version {
    fn into(self) -> String {
        format!("{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl Into<Version> for &str {
    fn into(self) -> Version {
        let parts: Vec<&str> = self.split('.').collect();
        let major = parts.get(0).and_then(|s| s.parse().ok()).unwrap_or(0);
        let minor = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        let patch = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
        Version {
            major,
            minor,
            patch,
        }
    }
}

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
pub struct TableSchema {
    pub name: String,
    pub visibility: TableVisibility,
    pub entries: Entries,
    pub primary_key: EntrySchema,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum TableVisibility {
    Public,
    Private,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SubscriptionSchema {
    pub module_name: String,
    pub table_name: String,
    pub reducer_name: String,
    pub event: TableEvent,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub enum TableEvent {
    Insert,
    Update,
    Delete,
}
