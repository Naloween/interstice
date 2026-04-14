use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RawReducerContext {
    pub caller_node_id: String,
}

impl RawReducerContext {
    pub fn new(caller_node_id: String) -> Self {
        Self { caller_node_id }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RawQueryContext {
    pub caller_node_id: String,
}

impl RawQueryContext {
    pub fn new(caller_node_id: String) -> Self {
        Self { caller_node_id }
    }
}
