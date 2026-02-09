mod convert;
mod index_key;
mod row;
mod validate;

pub use index_key::IndexKey;
pub use validate::validate_value;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub enum IntersticeValue {
    Void,
    U8(u8),
    U32(u32),
    U64(u64),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    Bool(bool),
    String(String),

    Vec(Vec<IntersticeValue>),
    Option(Option<Box<IntersticeValue>>),
    Tuple(Vec<IntersticeValue>),

    Struct {
        name: String,
        fields: Vec<Field>,
    },

    Enum {
        name: String,
        variant: String,
        value: Box<IntersticeValue>,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Field {
    pub name: String,
    pub value: IntersticeValue,
}
