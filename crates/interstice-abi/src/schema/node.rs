use crate::ModuleSchema;

pub struct NodeSchema {
    pub name: String,
    pub adress: String,
    pub modules: Vec<ModuleSchema>,
}
