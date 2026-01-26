use serde::{Deserialize, Serialize};

use crate::entry::{Entries, EntrySchema};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TableSchema {
    pub name: String,
    pub visibility: TableVisibility,
    pub entries: Entries,
    pub primary_key: EntrySchema,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub enum TableVisibility {
    Public,
    Private,
}
