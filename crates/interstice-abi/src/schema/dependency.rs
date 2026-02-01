use crate::Version;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Dependency {
    pub module_name: String,
    pub version: Version,
}
