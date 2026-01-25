use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReducerContext {
    pub current: CurrentModuleContext,
}

impl ReducerContext {
    pub fn new() -> Self {
        Self {
            current: CurrentModuleContext {},
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CurrentModuleContext {}
