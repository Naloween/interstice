use crate::IntersticeValue;

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
