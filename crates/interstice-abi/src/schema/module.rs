use crate::{
    ABI_VERSION,
    reducer::{ReducerSchema, SubscriptionSchema},
    table::{TableSchema, TableVisibility},
    version::Version,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModuleSchema {
    pub abi_version: u16,
    pub name: String,
    pub version: Version,
    pub reducers: Vec<ReducerSchema>,
    pub tables: Vec<TableSchema>,
    pub subscriptions: Vec<SubscriptionSchema>,
}

impl ModuleSchema {
    pub fn new(
        name: impl Into<String>,
        version: Version,
        reducers: Vec<ReducerSchema>,
        tables: Vec<TableSchema>,
        subscriptions: Vec<SubscriptionSchema>,
    ) -> Self {
        Self {
            abi_version: ABI_VERSION,
            name: name.into(),
            version,
            reducers,
            tables,
            subscriptions,
        }
    }

    pub fn to_public(&self) -> Self {
        let mut tables = Vec::new();
        for table_schema in &self.tables {
            if table_schema.visibility == TableVisibility::Public {
                tables.push(table_schema.clone());
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
            name: self.name.clone(),
            version: self.version.clone(),
            reducers,
            tables,
            subscriptions: Vec::new(),
        }
    }

    pub fn from_toml_string(toml_string: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(toml_string)
    }

    pub fn to_toml_string(&self) -> Result<String, toml::ser::Error> {
        toml::to_string(&self)
    }
}
