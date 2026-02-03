use crate::ModuleSchema;

#[derive(Debug, Clone)]
pub struct NodeSchema {
    pub id: String,
    pub name: String,
    pub adress: String,
    pub modules: Vec<ModuleSchema>,
}
