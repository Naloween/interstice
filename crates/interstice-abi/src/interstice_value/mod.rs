mod convert;
mod index_key;
mod row;
mod validate;

use std::fmt::{Display, Write};

pub use index_key::IndexKey;
pub use validate::{validate_value, validate_value_detailed};

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

    /// A `Vec<u8>`, stored contiguously. Semantically identical to a `Vec` of
    /// `U8` (it validates against `IntersticeType::Vec(U8)`), but encodes as
    /// one length + raw bytes instead of one tagged value per byte — the
    /// difference between ~1 ms and ~250 ms for a 4 MB texture upload.
    /// Appended last so existing encoded values keep their variant indices.
    Bytes(#[serde(with = "raw_bytes")] Vec<u8>),
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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Field {
    pub name: String,
    pub value: IntersticeValue,
}

impl Display for IntersticeValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.fmt_with_indent(f, 0)
    }
}

impl IntersticeValue {
    fn fmt_with_indent(&self, f: &mut std::fmt::Formatter<'_>, indent: usize) -> std::fmt::Result {
        let indent_str = "  ".repeat(indent);
        let next_indent_str = "  ".repeat(indent + 1);

        match self {
            IntersticeValue::Void => write!(f, "void"),
            IntersticeValue::U8(value) => write!(f, "{value}"),
            IntersticeValue::U32(value) => write!(f, "{value}"),
            IntersticeValue::U64(value) => write!(f, "{value}"),
            IntersticeValue::I32(value) => write!(f, "{value}"),
            IntersticeValue::I64(value) => write!(f, "{value}"),
            IntersticeValue::F32(value) => write!(f, "{value}"),
            IntersticeValue::F64(value) => write!(f, "{value}"),
            IntersticeValue::Bool(value) => write!(f, "{value}"),
            IntersticeValue::String(value) => write!(f, "\"{value}\""),
            IntersticeValue::Bytes(bytes) => write!(f, "<{} bytes>", bytes.len()),
            IntersticeValue::Vec(values) => {
                write!(f, "[\n")?;
                for value in values {
                    write!(f, "{next_indent_str}")?;
                    value.fmt_with_indent(f, indent + 1)?;
                    write!(f, ",\n")?;
                }
                write!(f, "{indent_str}]")
            }
            IntersticeValue::Option(opt) => {
                if let Some(inner) = opt {
                    write!(f, "Some(")?;
                    inner.fmt_with_indent(f, indent)?;
                    write!(f, ")")
                } else {
                    write!(f, "None")
                }
            }
            IntersticeValue::Tuple(values) => {
                let values_str = values
                    .iter()
                    .map(|v| {
                        let mut buf = String::new();
                        let _ = write!(buf, "{}", v);
                        buf
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "({values_str})")
            }
            IntersticeValue::Struct { name, fields } => {
                write!(f, "{} {{\n", name)?;
                for field in fields {
                    write!(f, "{next_indent_str}{}: ", field.name)?;
                    field.value.fmt_with_indent(f, indent + 1)?;
                    write!(f, ",\n")?;
                }
                write!(f, "{indent_str}}}")
            }
            IntersticeValue::Enum {
                name,
                variant,
                value,
            } => {
                write!(f, "{}::{}(", name, variant)?;
                value.fmt_with_indent(f, indent)?;
                write!(f, ")")
            }
        }
    }
}
