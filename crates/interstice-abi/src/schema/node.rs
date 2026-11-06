use serde::{Deserialize, Serialize};

use crate::{ModuleSchema, ModuleVisibility};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NodeSchema {
    pub name: String,
    pub address: String,
    pub modules: Vec<ModuleSchema>,
}

impl NodeSchema {
    /// Public modules as a non-owner peer may see them (DESIGN §7.4).
    pub fn to_remote(self) -> Self {
        Self {
            name: self.name,
            address: self.address,
            modules: self
                .modules
                .into_iter()
                .filter(|m| m.visibility == ModuleVisibility::Public)
                .map(|m| m.to_remote())
                .collect(),
        }
    }

    pub fn to_public(self) -> Self {
        let mut modules = Vec::new();
        for module in self.modules {
            if module.visibility == ModuleVisibility::Public {
                modules.push(module.to_public());
            }
        }
        Self {
            name: self.name,
            address: self.address,
            modules,
        }
    }

    pub fn from_toml_string(toml_string: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(toml_string)
    }

    pub fn to_toml_string(&self) -> Result<String, toml::ser::Error> {
        toml::to_string(&self)
    }
}
