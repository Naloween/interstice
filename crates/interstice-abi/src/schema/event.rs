use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub enum SubscriptionEventSchema {
    Insert {
        module_name: String,
        table_name: String,
    },
    Update {
        module_name: String,
        table_name: String,
    },
    Delete {
        module_name: String,
        table_name: String,
    },
    Init,
    Input,
    Render,
}
