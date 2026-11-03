use std::any::Any;

use crate::{IntersticeAbiError, IntersticeValue};

impl<T> Into<IntersticeValue> for Vec<T>
where
    T: Into<IntersticeValue> + 'static,
{
    fn into(self) -> IntersticeValue {
        // Byte vectors take the compact representation.
        let boxed: Box<dyn Any> = Box::new(self);
        match boxed.downcast::<Vec<u8>>() {
            Ok(bytes) => IntersticeValue::Bytes(*bytes),
            Err(other) => {
                let this = *other.downcast::<Vec<T>>().expect("Vec<T> round-trips through Any");
                IntersticeValue::Vec(this.into_iter().map(|x| x.into()).collect())
            }
        }
    }
}

impl<T> TryFrom<IntersticeValue> for Vec<T>
where
    T: TryFrom<IntersticeValue> + 'static,
    T::Error: std::fmt::Display,
{
    type Error = IntersticeAbiError;

    fn try_from(value: IntersticeValue) -> Result<Self, Self::Error> {
        let elements = match value {
            IntersticeValue::Bytes(bytes) => {
                let boxed: Box<dyn Any> = Box::new(bytes);
                match boxed.downcast::<Vec<T>>() {
                    Ok(v) => return Ok(*v),
                    // Bytes into a non-`u8` element type: convert per element
                    // (fails unless `T` accepts a `U8`).
                    Err(other) => {
                        let bytes = *other.downcast::<Vec<u8>>().expect("Vec<u8> round-trips through Any");
                        bytes.into_iter().map(IntersticeValue::U8).collect()
                    }
                }
            }
            IntersticeValue::Vec(v) => v,
            _ => {
                return Err(IntersticeAbiError::ConversionError(
                    "Expected IntersticeValue::Vec".into(),
                ));
            }
        };
        elements
            .into_iter()
            .map(|x| {
                T::try_from(x).map_err(|e| {
                    IntersticeAbiError::ConversionError(format!(
                        "Vec element conversion failed: {}",
                        e
                    ))
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::{IntersticeType, IntersticeValue, validate_value};
    use std::collections::HashMap;

    #[test]
    fn byte_vectors_are_compact_and_round_trip() {
        let v: IntersticeValue = vec![1u8, 2, 255].into();
        assert_eq!(v, IntersticeValue::Bytes(vec![1, 2, 255]));
        assert!(validate_value(&v, &IntersticeType::Vec(Box::new(IntersticeType::U8)), &HashMap::new()));
        assert!(!validate_value(&v, &IntersticeType::Vec(Box::new(IntersticeType::U32)), &HashMap::new()));

        // Encoded form is the same as the old per-element one (postcard: len + raw bytes).
        let enc = postcard::to_allocvec(&v).unwrap();
        let dec: IntersticeValue = postcard::from_bytes(&enc).unwrap();
        assert_eq!(dec, v);
        assert_eq!(Vec::<u8>::try_from(dec).unwrap(), vec![1, 2, 255]);

        // Values stored before `Bytes` existed still convert.
        let old = IntersticeValue::Vec(vec![IntersticeValue::U8(7), IntersticeValue::U8(8)]);
        assert_eq!(Vec::<u8>::try_from(old).unwrap(), vec![7, 8]);

        // Other element types keep the general representation.
        let w: IntersticeValue = vec![1u32, 2].into();
        assert!(matches!(w, IntersticeValue::Vec(_)));
        assert_eq!(Vec::<u32>::try_from(w).unwrap(), vec![1, 2]);
    }
}
