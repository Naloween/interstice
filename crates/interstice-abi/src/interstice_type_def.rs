use crate::IntersticeType;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum IntersticeTypeDef {
    Struct {
        name: String,
        fields: Vec<FieldDef>,
    },
    Enum {
        name: String,
        variants: Vec<FieldDef>,
    },
}

impl IntersticeTypeDef {
    pub fn get_name(&self) -> &String {
        match &self {
            IntersticeTypeDef::Struct { name, .. } => name,
            IntersticeTypeDef::Enum { name, .. } => name,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FieldDef {
    pub name: String,
    pub field_type: IntersticeType,
}
