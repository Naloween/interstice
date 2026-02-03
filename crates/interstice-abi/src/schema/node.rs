use serde::{Deserialize, Serialize};

use crate::ModuleSchema;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NodeSchema {
    pub name: String,
    pub adress: String,
    pub modules: Vec<ModuleSchema>,
}

impl NodeSchema {
    pub fn from_toml_string(toml_string: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(toml_string)
    }

    pub fn to_toml_string(&self) -> Result<String, toml::ser::Error> {
        toml::to_string(&self)
    }
}
