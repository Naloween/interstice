mod convert;
mod index_key;
mod row;
mod validate;

use std::fmt::{Display, Write};

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
