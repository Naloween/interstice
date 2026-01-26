use crate::IntersticeType;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EntrySchema {
    pub name: String,
    pub value_type: IntersticeType,
}
pub type Entries = Vec<EntrySchema>;
