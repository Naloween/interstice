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

impl PartialOrd for IndexKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for IndexKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use IndexKey::*;
        let rank = |key: &IndexKey| match key {
            U8(_) => 0,
            U32(_) => 1,
            U64(_) => 2,
            I32(_) => 3,
            I64(_) => 4,
            Bool(_) => 5,
            String(_) => 6,
            Option(_) => 7,
            Tuple(_) => 8,
        };

        let self_rank = rank(self);
        let other_rank = rank(other);
        if self_rank != other_rank {
            return self_rank.cmp(&other_rank);
        }

        match (self, other) {
            (U8(a), U8(b)) => a.cmp(b),
            (U32(a), U32(b)) => a.cmp(b),
            (U64(a), U64(b)) => a.cmp(b),
            (I32(a), I32(b)) => a.cmp(b),
            (I64(a), I64(b)) => a.cmp(b),
            (Bool(a), Bool(b)) => a.cmp(b),
            (String(a), String(b)) => a.cmp(b),
            (Option(a), Option(b)) => match (a, b) {
                (None, None) => std::cmp::Ordering::Equal,
                (None, Some(_)) => std::cmp::Ordering::Less,
                (Some(_), None) => std::cmp::Ordering::Greater,
                (Some(a), Some(b)) => a.cmp(b),
            },
            (Tuple(a), Tuple(b)) => {
                for (left, right) in a.iter().zip(b.iter()) {
                    let ord = left.cmp(right);
                    if ord != std::cmp::Ordering::Equal {
                        return ord;
                    }
                }
                a.len().cmp(&b.len())
            }
            _ => std::cmp::Ordering::Equal,
        }
    }
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
