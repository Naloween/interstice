use serde::{Deserialize, Serialize};

use super::IntersticeValue;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub enum IndexKey {
    U8(u8),
    U32(u32),
    U64(u64),
    I32(i32),
    I64(i64),
    Bool(bool),
    String(String),
    Option(Option<Box<IndexKey>>),
    Tuple(Vec<IndexKey>),
}

impl TryFrom<&IntersticeValue> for IndexKey {
    type Error = String;

    fn try_from(value: &IntersticeValue) -> Result<Self, Self::Error> {
        match value {
            IntersticeValue::U8(v) => Ok(IndexKey::U8(*v)),
            IntersticeValue::U32(v) => Ok(IndexKey::U32(*v)),
            IntersticeValue::U64(v) => Ok(IndexKey::U64(*v)),
            IntersticeValue::I32(v) => Ok(IndexKey::I32(*v)),
            IntersticeValue::I64(v) => Ok(IndexKey::I64(*v)),
            IntersticeValue::Bool(v) => Ok(IndexKey::Bool(*v)),
            IntersticeValue::String(v) => Ok(IndexKey::String(v.clone())),
            IntersticeValue::Option(v) => match v {
                Some(inner) => Ok(IndexKey::Option(Some(Box::new(IndexKey::try_from(
                    inner.as_ref(),
                )?)))),
                None => Ok(IndexKey::Option(None)),
            },
            IntersticeValue::Tuple(items) => {
                let mut converted = Vec::with_capacity(items.len());
                for item in items {
                    converted.push(IndexKey::try_from(item)?);
                }
                Ok(IndexKey::Tuple(converted))
            }
            IntersticeValue::Void => Err("Void value cannot be used as an index key".to_string()),
            IntersticeValue::F32(_) | IntersticeValue::F64(_) => {
                Err("Float values cannot be used as index keys".to_string())
            }
            IntersticeValue::Vec(_)
            | IntersticeValue::Struct { .. }
            | IntersticeValue::Enum { .. } => Err(
                "Only primitive, Option, and Tuple values are supported as index keys".to_string(),
            ),
        }
    }
}

impl TryFrom<IntersticeValue> for IndexKey {
    type Error = String;

    fn try_from(value: IntersticeValue) -> Result<Self, Self::Error> {
        IndexKey::try_from(&value)
    }
}

impl From<IndexKey> for IntersticeValue {
    fn from(value: IndexKey) -> Self {
        match value {
            IndexKey::U8(v) => IntersticeValue::U8(v),
            IndexKey::U32(v) => IntersticeValue::U32(v),
            IndexKey::U64(v) => IntersticeValue::U64(v),
            IndexKey::I32(v) => IntersticeValue::I32(v),
            IndexKey::I64(v) => IntersticeValue::I64(v),
            IndexKey::Bool(v) => IntersticeValue::Bool(v),
            IndexKey::String(v) => IntersticeValue::String(v),
            IndexKey::Option(v) => IntersticeValue::Option(v.map(|inner| {
                let converted: IntersticeValue = (*inner).into();
                Box::new(converted)
            })),
            IndexKey::Tuple(items) => {
                IntersticeValue::Tuple(items.into_iter().map(IntersticeValue::from).collect())
            }
        }
    }
}
