use serde::{Deserialize, Serialize};

use crate::interstice_type_def::FieldDef;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReducerSchema {
    pub name: String,
    pub arguments: Vec<FieldDef>,
}

impl ReducerSchema {
    pub fn new(name: impl Into<String>, arguments: Vec<FieldDef>) -> Self {
        Self {
            name: name.into(),
            arguments,
        }
    }
}
