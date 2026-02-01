use crate::{
    ABI_VERSION, Authority, Dependency, IntersticeType, ReducerSchema, SubscriptionSchema,
    TableSchema, TableVisibility, Version, interstice_type_def::IntersticeTypeDef,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModuleSchema {
    pub abi_version: u16,
    pub name: String,
    pub version: Version,
    pub reducers: Vec<ReducerSchema>,
    pub tables: Vec<TableSchema>,
    pub subscriptions: Vec<SubscriptionSchema>,
    pub type_definitions: HashMap<String, IntersticeTypeDef>,
    pub authorities: Vec<Authority>,
    pub dependencies: Vec<Dependency>,
}

impl ModuleSchema {
    pub fn new(
        name: impl Into<String>,
        version: Version,
        reducers: Vec<ReducerSchema>,
        tables: Vec<TableSchema>,
        subscriptions: Vec<SubscriptionSchema>,
        type_definitions: HashMap<String, IntersticeTypeDef>,
        authorities: Vec<Authority>,
        dependencies: Vec<Dependency>,
    ) -> Self {
        Self {
            abi_version: ABI_VERSION,
            name: name.into(),
            version,
            reducers,
            tables,
            subscriptions,
            type_definitions,
            authorities,
            dependencies,
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
            version: self.version,
            reducers,
            tables,
            subscriptions: Vec::new(),
            type_definitions,
            authorities: self.authorities,
            dependencies: self.dependencies,
        }
    }

    pub fn from_toml_string(toml_string: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(toml_string)
    }

    pub fn to_toml_string(&self) -> Result<String, toml::ser::Error> {
        toml::to_string(&self)
    }
}
