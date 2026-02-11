use crate::NodeSelection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub enum SubscriptionEventSchema {
    Insert {
        node_selection: NodeSelection,
        module_name: String,
        table_name: String,
    },
    Update {
        node_selection: NodeSelection,
        module_name: String,
        table_name: String,
    },
    Delete {
        node_selection: NodeSelection,
        module_name: String,
        table_name: String,
    },
    Init,
    Load,
    Input,
    Render,
    File {
        path: String,
        recursive: bool,
    },
    ModulePublish,
    ModuleRemove,
}
