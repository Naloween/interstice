use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReducerContextCurrentModuleReducers {}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReducerContextCurrentModuleTables {}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReducerContextCurrentModule {
    pub tables: ReducerContextCurrentModuleTables,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReducerContext {
    pub current: ReducerContextCurrentModule,
}

impl ReducerContext {
    pub fn new() -> Self {
        Self {
            current: ReducerContextCurrentModule {
                tables: ReducerContextCurrentModuleTables {},
            },
        }
    }
}
