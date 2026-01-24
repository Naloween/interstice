//! Type conversions and serialization for the SDK
//!
//! This module provides traits and implementations for converting between
//! Rust types and IntersticeValue (from the ABI).

use interstice_abi::IntersticeValue;
use std::fmt::Debug;

/// Result type for SDK type operations
pub type Result<T> = std::result::Result<T, String>;

/// Trait for types that can be serialized to/from IntersticeValue.
///
/// Implement this for custom types used in tables or as reducer arguments.
/// This bridges Rust's strong typing with Interstice's dynamic value system.
pub trait Serialize: Sized + Debug + Clone {
    /// Convert from IntersticeValue to this type
    fn from_value(v: IntersticeValue) -> std::result::Result<Self, String>;

    /// Convert from this type to IntersticeValue
    fn to_value(&self) -> IntersticeValue;
}

// Implementations for built-in types
impl Serialize for String {
    fn from_value(v: IntersticeValue) -> std::result::Result<Self, String> {
        match v {
            IntersticeValue::String(s) => Ok(s),
            _ => Err(format!("Expected String, got {:?}", v)),
        }
    }

    fn to_value(&self) -> IntersticeValue {
        IntersticeValue::String(self.clone())
    }
}

impl Serialize for u64 {
    fn from_value(v: IntersticeValue) -> std::result::Result<Self, String> {
        match v {
            IntersticeValue::U64(n) => Ok(n),
            _ => Err(format!("Expected u64, got {:?}", v)),
        }
    }

    fn to_value(&self) -> IntersticeValue {
        IntersticeValue::U64(*self)
    }
}

impl Serialize for u32 {
    fn from_value(v: IntersticeValue) -> std::result::Result<Self, String> {
        match v {
            IntersticeValue::U32(n) => Ok(n),
            _ => Err(format!("Expected u32, got {:?}", v)),
        }
    }

    fn to_value(&self) -> IntersticeValue {
        IntersticeValue::U32(*self)
    }
}

impl Serialize for i64 {
    fn from_value(v: IntersticeValue) -> std::result::Result<Self, String> {
        match v {
            IntersticeValue::I64(n) => Ok(n),
            _ => Err(format!("Expected i64, got {:?}", v)),
        }
    }

    fn to_value(&self) -> IntersticeValue {
        IntersticeValue::I64(*self)
    }
}

impl Serialize for i32 {
    fn from_value(v: IntersticeValue) -> std::result::Result<Self, String> {
        match v {
            IntersticeValue::I32(n) => Ok(n),
            _ => Err(format!("Expected i32, got {:?}", v)),
        }
    }

    fn to_value(&self) -> IntersticeValue {
        IntersticeValue::I32(*self)
    }
}

impl Serialize for bool {
    fn from_value(v: IntersticeValue) -> std::result::Result<Self, String> {
        match v {
            IntersticeValue::Bool(b) => Ok(b),
            _ => Err(format!("Expected bool, got {:?}", v)),
        }
    }

    fn to_value(&self) -> IntersticeValue {
        IntersticeValue::Bool(*self)
    }
}

impl Serialize for f32 {
    fn from_value(v: IntersticeValue) -> std::result::Result<Self, String> {
        match v {
            IntersticeValue::F32(f) => Ok(f),
            _ => Err(format!("Expected f32, got {:?}", v)),
        }
    }

    fn to_value(&self) -> IntersticeValue {
        IntersticeValue::F32(*self)
    }
}

impl Serialize for f64 {
    fn from_value(v: IntersticeValue) -> std::result::Result<Self, String> {
        match v {
            IntersticeValue::F64(f) => Ok(f),
            _ => Err(format!("Expected f64, got {:?}", v)),
        }
    }

    fn to_value(&self) -> IntersticeValue {
        IntersticeValue::F64(*self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_roundtrip() {
        let s = "hello".to_string();
        let v = s.to_value();
        let s2 = String::from_value(v).unwrap();
        assert_eq!(s, s2);
    }

    #[test]
    fn test_u64_roundtrip() {
        let n = 42u64;
        let v = n.to_value();
        let n2 = u64::from_value(v).unwrap();
        assert_eq!(n, n2);
    }

    #[test]
    fn test_f32_roundtrip() {
        let f = 3.14f32;
        let v = f.to_value();
        let f2 = f32::from_value(v).unwrap();
        assert!((f - f2).abs() < 0.001);
    }

    #[test]
    fn test_type_mismatch_error() {
        let v = IntersticeValue::String("hello".to_string());
        assert!(u64::from_value(v).is_err());
    }
}
