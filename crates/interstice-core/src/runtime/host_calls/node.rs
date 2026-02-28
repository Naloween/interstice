use crate::runtime::Runtime;

impl Runtime {
    pub fn handle_current_node_id(&self) -> String {
        self.node_id.to_string()
    }
}
