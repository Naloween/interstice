use crate::{
    ABI_VERSION, Authority, IntersticeType, ModuleDependency, NodeDependency, ReducerSchema,
    SubscriptionSchema, TableSchema, TableVisibility, Version,
    interstice_type_def::IntersticeTypeDef,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub enum ModuleVisibility {
    // Visible to all other nodes
    Public,
    // Only visible for local modules on the current node
    Private,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModuleSchema {
    pub abi_version: u16,
    pub name: String,
    pub version: Version,
    pub visibility: ModuleVisibility,
    pub reducers: Vec<ReducerSchema>,
    pub tables: Vec<TableSchema>,
    pub subscriptions: Vec<SubscriptionSchema>,
    pub type_definitions: HashMap<String, IntersticeTypeDef>,
    pub authorities: Vec<Authority>,
    pub module_dependencies: Vec<ModuleDependency>,
    pub node_dependencies: Vec<NodeDependency>,
}

impl ModuleSchema {
    pub fn new(
        name: impl Into<String>,
        version: Version,
        visibility: ModuleVisibility,
        reducers: Vec<ReducerSchema>,
        tables: Vec<TableSchema>,
        subscriptions: Vec<SubscriptionSchema>,
        type_definitions: HashMap<String, IntersticeTypeDef>,
        authorities: Vec<Authority>,
        module_dependencies: Vec<ModuleDependency>,
        node_dependencies: Vec<NodeDependency>,
    ) -> Self {
        Self {
            abi_version: ABI_VERSION,
            name: name.into(),
            visibility,
            version,
            reducers,
            tables,
            subscriptions,
            type_definitions,
            authorities,
            module_dependencies,
            node_dependencies,
        }
    }

    pub fn to_public(self) -> Self {
        let mut type_definitions = HashMap::new();
        let mut tables = Vec::new();
        for table_schema in &self.tables {
            if table_schema.visibility == TableVisibility::Public {
                tables.push(table_schema.clone());
                for field in &table_schema.fields {
                    if let IntersticeType::Named(type_name) = field.field_type.clone() {
                        if !type_definitions.contains_key(&type_name) {
                            let type_def = self.type_definitions.get(&type_name).unwrap().clone();
                            type_definitions.insert(type_name, type_def);
                        }
                    }
                }
            }
        }
        let mut reducers = Vec::new();
        for reducer_schema in &self.reducers {
            let mut add_reducer = true;
            for subscription in &self.subscriptions {
                if subscription.reducer_name == reducer_schema.name {
                    add_reducer = false;
                    break;
                }
            }
            if add_reducer {
                reducers.push(reducer_schema.clone());
            }
        }

        Self {
            abi_version: self.abi_version,
            name: self.name,
            visibility: self.visibility,
            version: self.version,
            reducers,
            tables,
            subscriptions: Vec::new(),
            type_definitions,
            authorities: self.authorities,
            module_dependencies: self.module_dependencies,
            node_dependencies: self.node_dependencies,
        }
    }

    pub fn from_toml_string(toml_string: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(toml_string)
    }

    pub fn to_toml_string(&self) -> Result<String, toml::ser::Error> {
        toml::to_string(&self)
    }
}
