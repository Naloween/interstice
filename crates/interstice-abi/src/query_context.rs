use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueryContextCurrentModuleTables {}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueryContextCurrentModule {
    pub tables: QueryContextCurrentModuleTables,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueryContext {
    pub current: QueryContextCurrentModule,
}

impl QueryContext {
    pub fn new() -> Self {
        Self {
            current: QueryContextCurrentModule {
                tables: QueryContextCurrentModuleTables {},
            },
        }
    }
}
