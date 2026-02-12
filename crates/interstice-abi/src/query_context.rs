use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueryContextCurrentModuleTables {}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueryContextCurrentModule {
    pub tables: QueryContextCurrentModuleTables,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueryContext {
    pub caller_node_id: String,
    pub current: QueryContextCurrentModule,
}

impl QueryContext {
    pub fn new(caller_node_id: String) -> Self {
        Self {
            caller_node_id,
            current: QueryContextCurrentModule {
                tables: QueryContextCurrentModuleTables {},
            },
        }
    }
}
