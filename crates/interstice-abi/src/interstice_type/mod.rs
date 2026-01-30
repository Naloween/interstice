use crate::error::IntersticeAbiError;
use serde::{Deserialize, Serialize};
use std::{
    fmt::{Display, Formatter},
    str::FromStr,
};

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
    Tuple(Vec<IntersticeType>),
    Named(String), // reference to user-defined struct/enum
}

impl Display for IntersticeType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            IntersticeType::Void => write!(f, "()"),
            IntersticeType::U32 => write!(f, "u32"),
            IntersticeType::U64 => write!(f, "u64"),
            IntersticeType::I32 => write!(f, "i32"),
            IntersticeType::I64 => write!(f, "i64"),
            IntersticeType::F32 => write!(f, "f32"),
            IntersticeType::F64 => write!(f, "f64"),
            IntersticeType::Bool => write!(f, "bool"),
            IntersticeType::String => write!(f, "String"),

            IntersticeType::Vec(inner) => write!(f, "Vec<{}>", inner),
            IntersticeType::Option(inner) => write!(f, "Option<{}>", inner),

            IntersticeType::Tuple(types) => {
                write!(f, "(")?;
                for (i, ty) in types.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{ty}")?;
                }
                write!(f, ")")
            }

            IntersticeType::Named(s) => write!(f, "{s}"),
        }
    }
}
impl FromStr for IntersticeType {
    type Err = IntersticeAbiError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut p = Parser::new(s);
        let ty = p.parse_type()?;
        p.consume_ws();
        if !p.is_eof() {
            return Err(IntersticeAbiError::ConversionError(format!(
                "Unexpected trailing input: '{}'",
                p.rest()
            )));
        }
        Ok(ty)
    }
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn rest(&self) -> &str {
        &self.input[self.pos..]
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.input.len()
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn bump(&mut self) {
        if let Some(c) = self.peek() {
            self.pos += c.len_utf8();
        }
    }

    fn consume_ws(&mut self) {
        while matches!(self.peek(), Some(c) if c.is_whitespace()) {
            self.bump();
        }
    }

    fn eat(&mut self, ch: char) -> Result<(), IntersticeAbiError> {
        self.consume_ws();
        match self.peek() {
            Some(c) if c == ch => {
                self.bump();
                Ok(())
            }
            _ => Err(IntersticeAbiError::ConversionError(format!(
                "Expected '{}' at '{}'",
                ch,
                self.rest()
            ))),
        }
    }

    fn parse_ident(&mut self) -> Result<String, IntersticeAbiError> {
        self.consume_ws();
        let mut len = 0;
        for c in self.rest().chars() {
            if c.is_alphanumeric() || c == '_' {
                len += c.len_utf8();
            } else {
                break;
            }
        }
        if len == 0 {
            return Err(IntersticeAbiError::ConversionError(format!(
                "Expected identifier at '{}'",
                self.rest()
            )));
        }
        let ident = self.rest()[..len].to_string();
        self.pos += len;
        Ok(ident)
    }

    fn parse_type(&mut self) -> Result<IntersticeType, IntersticeAbiError> {
        self.consume_ws();

        // ---- Tuple or unit ----
        if self.peek() == Some('(') {
            self.bump(); // '('
            self.consume_ws();

            if self.peek() == Some(')') {
                self.bump();
                return Ok(IntersticeType::Void);
            }

            let mut elems = Vec::new();
            loop {
                elems.push(self.parse_type()?);
                self.consume_ws();
                match self.peek() {
                    Some(',') => {
                        self.bump();
                    }
                    Some(')') => {
                        self.bump();
                        break;
                    }
                    _ => {
                        return Err(IntersticeAbiError::ConversionError(format!(
                            "Expected ',' or ')' in tuple at '{}'",
                            self.rest()
                        )));
                    }
                }
            }
            return Ok(IntersticeType::Tuple(elems));
        }

        // ---- Identifier ----
        let ident = self.parse_ident()?;

        // ---- Primitives ----
        let primitive = match ident.as_str() {
            "u32" => Some(IntersticeType::U32),
            "u64" => Some(IntersticeType::U64),
            "i32" => Some(IntersticeType::I32),
            "i64" => Some(IntersticeType::I64),
            "f32" => Some(IntersticeType::F32),
            "f64" => Some(IntersticeType::F64),
            "bool" => Some(IntersticeType::Bool),
            "String" => Some(IntersticeType::String),
            "Vec" | "Option" => None,
            _ => None,
        };

        // ---- Generics like Vec<T> / Option<T> ----
        self.consume_ws();
        if self.peek() == Some('<') {
            self.bump(); // '<'
            let inner = self.parse_type()?;
            self.consume_ws();
            self.eat('>')?;

            return match ident.as_str() {
                "Vec" => Ok(IntersticeType::Vec(Box::new(inner))),
                "Option" => Ok(IntersticeType::Option(Box::new(inner))),
                _ => Err(IntersticeAbiError::ConversionError(format!(
                    "Unknown generic type '{}'",
                    ident
                ))),
            };
        }

        if let Some(p) = primitive {
            return Ok(p);
        }

        // ---- Named user type ----
        Ok(IntersticeType::Named(ident))
    }
}
