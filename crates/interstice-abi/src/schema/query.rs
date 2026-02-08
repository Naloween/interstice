use serde::{Deserialize, Serialize};

use crate::{FieldDef, IntersticeType};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QuerySchema {
    pub name: String,
    pub arguments: Vec<FieldDef>,
    pub return_type: IntersticeType,
}

impl QuerySchema {
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
