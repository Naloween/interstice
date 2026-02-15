use crate::{IntersticeAbiError, IntersticeValue};

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

impl<T> std::convert::TryFrom<IntersticeValue> for Option<T>
where
    T: TryFrom<IntersticeValue, Error = IntersticeAbiError>,
{
    type Error = IntersticeAbiError;

    fn try_from(value: IntersticeValue) -> Result<Self, Self::Error> {
        match value {
            IntersticeValue::Option(opt) => match opt {
                Some(inner) => T::try_from(*inner).map(Some),
                None => Ok(None),
            },
            IntersticeValue::Void => Ok(None),
            other => Err(IntersticeAbiError::ConversionError(format!(
                "Expected IntersticeValue::Option or IntersticeValue::Void, got {:?}",
                other
            ))),
        }
    }
}
