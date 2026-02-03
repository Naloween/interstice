use crate::ModuleSchema;

#[derive(Debug, Clone)]
pub struct NodeSchema {
    pub modules: Vec<ModuleSchema>,
}
