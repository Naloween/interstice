use std::collections::HashMap;

use crate::{
    IntersticeType, Row, error::IntersticeAbiError, interstice_type_def::IntersticeTypeDef,
};
use serde::{Deserialize, Serialize};

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

impl IntersticeValue {
    pub fn from_row(row: &Row) -> Self {
        let mut values = Vec::with_capacity(1 + row.entries.len());
        values.push(row.primary_key.clone());
        values.extend_from_slice(&row.entries);
        IntersticeValue::Vec(values)
    }
}

pub fn validate_value(
    value: &IntersticeValue,
    ty: &IntersticeType,
    type_definitions: &HashMap<String, IntersticeTypeDef>,
) -> bool {
    match value {
        IntersticeValue::Void => {
            if let IntersticeType::Void = ty {
                true
            } else {
                false
            }
        }
        IntersticeValue::Bool(_) => {
            if let IntersticeType::Bool = ty {
                true
            } else {
                false
            }
        }
        IntersticeValue::U32(_) => {
            if let IntersticeType::U32 = ty {
                true
            } else {
                false
            }
        }
        IntersticeValue::U64(_) => {
            if let IntersticeType::U64 = ty {
                true
            } else {
                false
            }
        }
        IntersticeValue::I32(_) => {
            if let IntersticeType::I32 = ty {
                true
            } else {
                false
            }
        }
        IntersticeValue::I64(_) => {
            if let IntersticeType::I64 = ty {
                true
            } else {
                false
            }
        }
        IntersticeValue::F32(_) => {
            if let IntersticeType::F32 = ty {
                true
            } else {
                false
            }
        }
        IntersticeValue::F64(_) => {
            if let IntersticeType::F64 = ty {
                true
            } else {
                false
            }
        }
        IntersticeValue::String(_) => {
            if let IntersticeType::String = ty {
                true
            } else {
                false
            }
        }
        IntersticeValue::Vec(v) => {
            if let IntersticeType::Vec(inner) = ty {
                v.iter().all(|x| validate_value(x, inner, type_definitions))
            } else {
                false
            }
        }
        IntersticeValue::Option(None) => {
            if let IntersticeType::Option(_) = ty {
                true
            } else {
                false
            }
        }
        IntersticeValue::Option(Some(v)) => {
            if let IntersticeType::Option(inner) = ty {
                validate_value(v, inner, type_definitions)
            } else {
                false
            }
        }
        IntersticeValue::Tuple(interstice_values) => {
            if let IntersticeType::Tuple(inner) = ty {
                inner
                    .iter()
                    .zip(interstice_values)
                    .all(|(inner_ty, inner_val)| {
                        validate_value(inner_val, inner_ty, type_definitions)
                    })
            } else {
                false
            }
        }
        IntersticeValue::Struct { name, fields } => {
            if let IntersticeType::Named(type_name) = ty {
                if type_name != name {
                    return false;
                }
                if let Some(IntersticeTypeDef::Struct {
                    name: _name_def,
                    fields: fields_def,
                }) = type_definitions.get(type_name)
                {
                    // TODO: here the order of the field definition matter, make it not
                    fields.iter().zip(fields_def).all(|(field, field_def)| {
                        field.name == field_def.name
                            && validate_value(&field.value, &field_def.field_type, type_definitions)
                    })
                } else {
                    false
                }
            } else {
                false
            }
        }
        IntersticeValue::Enum {
            name,
            variant,
            value,
        } => {
            if let IntersticeType::Named(type_name) = ty {
                if type_name != name {
                    return false;
                }
                if let Some(IntersticeTypeDef::Enum {
                    name: _name_def,
                    variants,
                }) = type_definitions.get(type_name)
                {
                    for variant_def in variants {
                        if &variant_def.name == variant {
                            return validate_value(
                                value,
                                &variant_def.field_type,
                                type_definitions,
                            );
                        }
                    }
                    false
                } else {
                    false
                }
            } else {
                false
            }
        }
    }
}

// Base type conversions

impl Into<IntersticeValue> for () {
    fn into(self) -> IntersticeValue {
        IntersticeValue::Void
    }
}
impl TryInto<()> for IntersticeValue {
    type Error = IntersticeAbiError;

    fn try_into(self) -> Result<(), Self::Error> {
        if let IntersticeValue::Void = self {
            Ok(())
        } else {
            Err(IntersticeAbiError::ConversionError(
                "Expected IntersticeValue::Void".into(),
            ))
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

// Tuples implementations

macro_rules! impl_tuple_into_interstice {
    ( $( $name:ident ),+ ) => {
        impl<$( $name ),+> From<( $( $name ),+ )> for IntersticeValue
        where
            $( $name: Into<IntersticeValue> ),+
        {
            fn from(value: ( $( $name ),+ )) -> Self {
                let ( $( $name ),+ ) = value;
                IntersticeValue::Tuple(vec![
                    $( $name.into() ),+
                ])
            }
        }
    };
}

impl_tuple_into_interstice!(A, B);
impl_tuple_into_interstice!(A, B, C);
impl_tuple_into_interstice!(A, B, C, D);
impl_tuple_into_interstice!(A, B, C, D, E);
impl_tuple_into_interstice!(A, B, C, D, E, F);
impl_tuple_into_interstice!(A, B, C, D, E, F, G);
impl_tuple_into_interstice!(A, B, C, D, E, F, G, H);

macro_rules! count_idents {
    ($($idents:ident),*) => {
        <[()]>::len(&[$(count_idents!(@sub $idents)),*])
    };
    (@sub $ident:ident) => { () };
}

macro_rules! impl_tuple_tryfrom_interstice {
    ( $( $name:ident ),+ ) => {
        impl<$( $name ),+> TryFrom<IntersticeValue> for ( $( $name ),+ )
        where
            $( $name: TryFrom<IntersticeValue> ),+,
            $( <$name as TryFrom<IntersticeValue>>::Error: std::fmt::Display ),+
        {
            type Error = String;

            fn try_from(value: IntersticeValue) -> Result<Self, Self::Error> {
                match value {
                    IntersticeValue::Tuple(vec) => {
                        let expected = count_idents!( $( $name ),+ );
                        if vec.len() != expected {
                            return Err(format!(
                                "Tuple arity mismatch: expected {}, got {}",
                                expected,
                                vec.len()
                            ));
                        }

                        // We index instead of consuming iterator so order is explicit
                        let mut iter = vec.into_iter();
                        Ok((
                            $(
                                {
                                    let v = iter.next().unwrap();
                                    <$name as TryFrom<IntersticeValue>>::try_from(v)
                                        .map_err(|e| format!("Tuple element conversion failed: {}", e))?
                                }
                            ),+
                        ))
                    }
                    other => Err(format!("Expected Tuple, got {:?}", other)),
                }
            }
        }
    };
}

impl_tuple_tryfrom_interstice!(A, B);
impl_tuple_tryfrom_interstice!(A, B, C);
impl_tuple_tryfrom_interstice!(A, B, C, D);
impl_tuple_tryfrom_interstice!(A, B, C, D, E);
impl_tuple_tryfrom_interstice!(A, B, C, D, E, F);
impl_tuple_tryfrom_interstice!(A, B, C, D, E, F, G);
impl_tuple_tryfrom_interstice!(A, B, C, D, E, F, G, H);
