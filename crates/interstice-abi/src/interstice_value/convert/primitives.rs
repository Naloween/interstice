use crate::{IntersticeAbiError, IntersticeValue};

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

impl Into<IntersticeValue> for String {
    fn into(self) -> IntersticeValue {
        IntersticeValue::String(self)
    }
}
impl TryFrom<IntersticeValue> for String {
    type Error = IntersticeAbiError;

    fn try_from(value: IntersticeValue) -> Result<Self, Self::Error> {
        match value {
            IntersticeValue::String(s) => Ok(s),
            _ => Err(IntersticeAbiError::ConversionError(
                "Expected IntersticeValue::String".into(),
            )),
        }
    }
}

impl Into<IntersticeValue> for bool {
    fn into(self) -> IntersticeValue {
        IntersticeValue::Bool(self)
    }
}
impl TryFrom<IntersticeValue> for bool {
    type Error = IntersticeAbiError;

    fn try_from(value: IntersticeValue) -> Result<Self, Self::Error> {
        match value {
            IntersticeValue::Bool(b) => Ok(b),
            _ => Err(IntersticeAbiError::ConversionError(
                "Expected IntersticeValue::Bool".into(),
            )),
        }
    }
}

macro_rules! impl_to_interstice_value {
    ($variant:ident, $ty:ty) => {
        impl Into<IntersticeValue> for $ty {
            fn into(self) -> IntersticeValue {
                IntersticeValue::$variant(self)
            }
        }
    };
}

impl_to_interstice_value!(U8, u8);
impl_to_interstice_value!(U32, u32);
impl_to_interstice_value!(U64, u64);
impl_to_interstice_value!(I32, i32);
impl_to_interstice_value!(I64, i64);
impl_to_interstice_value!(F32, f32);
impl_to_interstice_value!(F64, f64);

macro_rules! impl_tryfrom_numeric {
    ($variant:ident, $ty:ty) => {
        impl TryFrom<IntersticeValue> for $ty {
            type Error = IntersticeAbiError;

            fn try_from(value: IntersticeValue) -> Result<Self, Self::Error> {
                match value {
                    IntersticeValue::$variant(v) => Ok(v),
                    _ => Err(IntersticeAbiError::ConversionError(format!(
                        "Expected IntersticeValue::{}",
                        stringify!($variant)
                    ))),
                }
            }
        }
    };
}

impl_tryfrom_numeric!(U8, u8);
impl_tryfrom_numeric!(U32, u32);
impl_tryfrom_numeric!(U64, u64);
impl_tryfrom_numeric!(I32, i32);
impl_tryfrom_numeric!(I64, i64);
impl_tryfrom_numeric!(F32, f32);
impl_tryfrom_numeric!(F64, f64);
