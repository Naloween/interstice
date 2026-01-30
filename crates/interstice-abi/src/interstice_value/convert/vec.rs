use crate::{IntersticeAbiError, IntersticeValue};

impl<T> Into<IntersticeValue> for Vec<T>
where
    T: Into<IntersticeValue>,
{
    fn into(self) -> IntersticeValue {
        let values = self.into_iter().map(|x| x.into()).collect();
        IntersticeValue::Vec(values)
    }
}
impl<T> TryFrom<IntersticeValue> for Vec<T>
where
    T: TryFrom<IntersticeValue>,
    T::Error: std::fmt::Display,
{
    type Error = IntersticeAbiError;

    fn try_from(value: IntersticeValue) -> Result<Self, Self::Error> {
        match value {
            IntersticeValue::Vec(v) => v
                .into_iter()
                .map(|x| {
                    T::try_from(x).map_err(|e| {
                        IntersticeAbiError::ConversionError(format!(
                            "Vec element conversion failed: {}",
                            e
                        ))
                    })
                })
                .collect(),
            _ => Err(IntersticeAbiError::ConversionError(
                "Expected IntersticeValue::Vec".into(),
            )),
        }
    }
}
