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
    pub caller_node_id: String,
    pub current: ReducerContextCurrentModule,
}

impl ReducerContext {
    pub fn new(caller_node_id: String) -> Self {
        Self {
            caller_node_id,
            current: ReducerContextCurrentModule {
                tables: ReducerContextCurrentModuleTables {},
            },
        }
    }
}
