use crate::IntersticeAbiError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub enum Authority {
    Gpu,
    Audio,
    Input,
    File,
}

impl Into<String> for Authority {
    fn into(self) -> String {
        match self {
            Authority::Gpu => "Gpu".into(),
            Authority::Audio => "Audio".into(),
            Authority::Input => "Input".into(),
            Authority::File => "File".into(),
        }
    }
}

impl TryInto<Authority> for String {
    type Error = IntersticeAbiError;

    fn try_into(self) -> Result<Authority, Self::Error> {
        match self.as_str() {
            "Gpu" => Ok(Authority::Gpu),
            "Audio" => Ok(Authority::Audio),
            "Input" => Ok(Authority::Input),
            "File" => Ok(Authority::File),
            _ => Err(IntersticeAbiError::ConversionError(
                "Couldn't convert String to Authority".into(),
            )),
        }
    }
}
