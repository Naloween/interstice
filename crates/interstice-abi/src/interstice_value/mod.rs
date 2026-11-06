mod convert;
mod index_key;
mod render;
mod row;
mod validate;

use std::fmt::Display;

pub use index_key::IndexKey;
pub use render::render;
pub use validate::{validate_value, validate_value_detailed};

use serde::{Deserialize, Serialize};

/// A value as it crosses the boundary, is stored, logged and sent between
/// nodes. Values are typed by the schema: structs and enums carry no names,
/// only positions (DESIGN §10.2).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub enum IntersticeValue {
    Void,
    Bool(bool),
    U8(u8),
    U32(u32),
    U64(u64),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    String(String),
    /// A `Vec<u8>`, stored contiguously: one length and the raw bytes
    /// instead of one tagged value per byte. Validates as `Vec(U8)`.
    Bytes(#[serde(with = "raw_bytes")] Vec<u8>),
    /// Handle to an immutable byte buffer held by the host for the call that
    /// created or received it (DESIGN §7.1). Validates as `Blob`.
    Blob(u64),

    Vec(Vec<IntersticeValue>),
    Option(Option<Box<IntersticeValue>>),
    Tuple(Vec<IntersticeValue>),
    /// A struct's fields, in declaration order.
    Struct(Vec<IntersticeValue>),
    /// An enum variant, by its index in declaration order, and its payload:
    /// `Void`, a `Tuple`, one value, or the struct `Enum::Variant`.
    Enum {
        variant: u32,
        value: Box<IntersticeValue>,
    },
}

/// (De)serialize a `Vec<u8>` as one byte string instead of a sequence of
/// `u8`s. Postcard's encoding is identical (varint length + raw bytes); only
/// the work differs — a memcpy instead of a visitor call per byte.
mod raw_bytes {
    use serde::{Deserializer, Serializer, de};

    pub fn serialize<S: Serializer>(bytes: &Vec<u8>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(bytes)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        struct V;
        impl<'de> de::Visitor<'de> for V {
            type Value = Vec<u8>;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a byte string")
            }
            fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Vec<u8>, E> {
                Ok(v.to_vec())
            }
            fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<Vec<u8>, E> {
                Ok(v)
            }
            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<u8>, A::Error> {
                let mut out = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(1 << 20));
                while let Some(b) = seq.next_element()? {
                    out.push(b);
                }
                Ok(out)
            }
        }
        d.deserialize_byte_buf(V)
    }
}

/// One line, without names (they live in the schema): `{1, "a"}` for a
/// struct, `#2(..)` for an enum variant. `render` adds the names.
impl Display for IntersticeValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let list = |f: &mut std::fmt::Formatter<'_>, items: &[IntersticeValue], open: &str, close: &str| {
            f.write_str(open)?;
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                write!(f, "{item}")?;
            }
            f.write_str(close)
        };
        match self {
            IntersticeValue::Void => write!(f, "()"),
            IntersticeValue::Bool(v) => write!(f, "{v}"),
            IntersticeValue::U8(v) => write!(f, "{v}"),
            IntersticeValue::U32(v) => write!(f, "{v}"),
            IntersticeValue::U64(v) => write!(f, "{v}"),
            IntersticeValue::I32(v) => write!(f, "{v}"),
            IntersticeValue::I64(v) => write!(f, "{v}"),
            IntersticeValue::F32(v) => write!(f, "{v}"),
            IntersticeValue::F64(v) => write!(f, "{v}"),
            IntersticeValue::String(v) => write!(f, "{v:?}"),
            IntersticeValue::Bytes(bytes) => write!(f, "<{} bytes>", bytes.len()),
            IntersticeValue::Blob(id) => write!(f, "<blob #{id}>"),
            IntersticeValue::Vec(items) => list(f, items, "[", "]"),
            IntersticeValue::Option(None) => write!(f, "None"),
            IntersticeValue::Option(Some(v)) => write!(f, "Some({v})"),
            IntersticeValue::Tuple(items) => list(f, items, "(", ")"),
            IntersticeValue::Struct(fields) => list(f, fields, "{", "}"),
            IntersticeValue::Enum { variant, value } => match value.as_ref() {
                IntersticeValue::Void => write!(f, "#{variant}"),
                value => write!(f, "#{variant}({value})"),
            },
        }
    }
}
