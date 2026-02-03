use crate::Version;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModuleDependency {
    pub module_name: String,
    pub version: Version,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NodeDependency {
    pub name: String,
    pub address: String,
}
