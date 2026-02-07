#[derive(Debug)]
pub enum IntersticeAbiError {
    ConversionError(String),
}

impl std::fmt::Display for IntersticeAbiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IntersticeAbiError::ConversionError(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for IntersticeAbiError {}
