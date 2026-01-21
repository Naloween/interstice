use crate::{reducer::ReducerSchema, types::ABI_VERSION};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModuleSchema {
    pub abi_version: u16,
    pub name: String,
    pub version: u32,
    pub reducers: Vec<ReducerSchema>,
    // tables will be added later
}

impl ModuleSchema {
    pub fn new(name: impl Into<String>, version: u32, reducers: Vec<ReducerSchema>) -> Self {
        Self {
            abi_version: ABI_VERSION,
            name: name.into(),
            version,
            reducers,
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, postcard::Error> {
        postcard::to_allocvec(self)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, postcard::Error> {
        postcard::from_bytes(bytes)
    }
}
