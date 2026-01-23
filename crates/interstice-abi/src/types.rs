use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Row {
    pub primary_key: IntersticeValue,
    pub entries: Vec<IntersticeValue>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum IntersticeType {
    Void,
    U32,
    U64,
    I32,
    I64,
    F32,
    F64,
    Bool,
    String,
    Vec(Box<IntersticeType>),
    Option(Box<IntersticeType>),
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub enum IntersticeValue {
    Void,
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
}

impl IntersticeValue {
    pub fn from_row(row: &Row) -> Self {
        let mut values = Vec::with_capacity(1 + row.entries.len());
        values.push(row.primary_key.clone());
        values.extend_from_slice(&row.entries);
        IntersticeValue::Vec(values)
    }
}

pub fn validate_value(value: &IntersticeValue, ty: &IntersticeType) -> bool {
    match (value, ty) {
        (IntersticeValue::Void, IntersticeType::Void) => true,
        (IntersticeValue::Bool(_), IntersticeType::Bool) => true,
        (IntersticeValue::U32(_), IntersticeType::U32) => true,
        (IntersticeValue::U64(_), IntersticeType::U64) => true,
        (IntersticeValue::I32(_), IntersticeType::I32) => true,
        (IntersticeValue::I64(_), IntersticeType::I64) => true,
        (IntersticeValue::F32(_), IntersticeType::F32) => true,
        (IntersticeValue::F64(_), IntersticeType::F64) => true,
        (IntersticeValue::String(_), IntersticeType::String) => true,
        (IntersticeValue::Vec(v), IntersticeType::Vec(inner)) => {
            v.iter().all(|x| validate_value(x, inner))
        }
        (IntersticeValue::Option(None), IntersticeType::Option(_)) => true,
        (IntersticeValue::Option(Some(v)), IntersticeType::Option(inner)) => {
            validate_value(v, inner)
        }
        _ => false,
    }
}

impl Into<IntersticeType> for String {
    fn into(self) -> IntersticeType {
        match self.as_str() {
            "()" => IntersticeType::Void,
            "Void" => IntersticeType::Void,
            "u32" => IntersticeType::U32,
            "u64" => IntersticeType::U64,
            "i32" => IntersticeType::I32,
            "i64" => IntersticeType::I64,
            "f32" => IntersticeType::F32,
            "f64" => IntersticeType::F64,
            "bool" => IntersticeType::Bool,
            "String" => IntersticeType::String,
            _ => panic!("Unknown IntersticeType string: {}", self),
        }
    }
}

impl Into<IntersticeValue> for () {
    fn into(self) -> IntersticeValue {
        IntersticeValue::Void
    }
}
impl Into<()> for IntersticeValue {
    fn into(self) -> () {
        if let IntersticeValue::Void = self {
            ()
        } else {
            panic!("Expected IntersticeValue::Void")
        }
    }
}

impl<T> Into<IntersticeValue> for Vec<T>
where
    T: Into<IntersticeValue>,
{
    fn into(self) -> IntersticeValue {
        let values = self.into_iter().map(|x| x.into()).collect();
        IntersticeValue::Vec(values)
    }
}
impl<T> Into<Vec<T>> for IntersticeValue
where
    T: From<IntersticeValue>,
{
    fn into(self) -> Vec<T> {
        if let IntersticeValue::Vec(v) = self {
            v.into_iter().map(|x| x.into()).collect()
        } else {
            panic!("Expected IntersticeValue::Vec")
        }
    }
}

impl<T> Into<IntersticeValue> for Option<T>
where
    T: Into<IntersticeValue>,
{
    fn into(self) -> IntersticeValue {
        match self {
            Some(v) => IntersticeValue::Option(Some(Box::new(v.into()))),
            None => IntersticeValue::Option(None),
        }
    }
}

impl Into<IntersticeValue> for String {
    fn into(self) -> IntersticeValue {
        IntersticeValue::String(self)
    }
}
impl Into<String> for IntersticeValue {
    fn into(self) -> String {
        if let IntersticeValue::String(s) = self {
            s
        } else {
            panic!("Expected IntersticeValue::String")
        }
    }
}

impl Into<IntersticeValue> for bool {
    fn into(self) -> IntersticeValue {
        IntersticeValue::Bool(self)
    }
}
impl Into<bool> for IntersticeValue {
    fn into(self) -> bool {
        if let IntersticeValue::Bool(s) = self {
            s
        } else {
            panic!("Expected IntersticeValue::bool")
        }
    }
}

impl Into<IntersticeValue> for u32 {
    fn into(self) -> IntersticeValue {
        IntersticeValue::U32(self)
    }
}
impl Into<u32> for IntersticeValue {
    fn into(self) -> u32 {
        if let IntersticeValue::U32(s) = self {
            s
        } else {
            panic!("Expected IntersticeValue::u32")
        }
    }
}

impl Into<IntersticeValue> for u64 {
    fn into(self) -> IntersticeValue {
        IntersticeValue::U64(self)
    }
}
impl Into<u64> for IntersticeValue {
    fn into(self) -> u64 {
        if let IntersticeValue::U64(s) = self {
            s
        } else {
            panic!("Expected IntersticeValue::u64")
        }
    }
}

impl Into<IntersticeValue> for i32 {
    fn into(self) -> IntersticeValue {
        IntersticeValue::I32(self)
    }
}
impl Into<i32> for IntersticeValue {
    fn into(self) -> i32 {
        if let IntersticeValue::I32(s) = self {
            s
        } else {
            panic!("Expected IntersticeValue::i32")
        }
    }
}

impl Into<IntersticeValue> for i64 {
    fn into(self) -> IntersticeValue {
        IntersticeValue::I64(self)
    }
}
impl Into<i64> for IntersticeValue {
    fn into(self) -> i64 {
        if let IntersticeValue::I64(s) = self {
            s
        } else {
            panic!("Expected IntersticeValue::i64")
        }
    }
}

impl Into<IntersticeValue> for f32 {
    fn into(self) -> IntersticeValue {
        IntersticeValue::F32(self)
    }
}
impl Into<f32> for IntersticeValue {
    fn into(self) -> f32 {
        if let IntersticeValue::F32(s) = self {
            s
        } else {
            panic!("Expected IntersticeValue::f32")
        }
    }
}

impl Into<IntersticeValue> for f64 {
    fn into(self) -> IntersticeValue {
        IntersticeValue::F64(self)
    }
}
impl Into<f64> for IntersticeValue {
    fn into(self) -> f64 {
        if let IntersticeValue::F64(s) = self {
            s
        } else {
            panic!("Expected IntersticeValue::f64")
        }
    }
}
