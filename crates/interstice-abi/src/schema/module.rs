use crate::{
    ABI_VERSION, Authority, IntersticeType, ModuleDependency, NodeDependency, QuerySchema,
    ReducerSchema, SubscriptionSchema, TableSchema, TableVisibility, Version,
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
    pub queries: Vec<QuerySchema>,
    pub tables: Vec<TableSchema>,
    pub subscriptions: Vec<SubscriptionSchema>,
    pub type_definitions: HashMap<String, IntersticeTypeDef>,
    pub authorities: Vec<Authority>,
    pub module_dependencies: Vec<ModuleDependency>,
    pub node_dependencies: Vec<NodeDependency>,
}

impl ModuleSchema {
    pub fn empty() -> Self {
        Self {
            abi_version: 0,
            name: "".into(),
            version: Version {
                major: 0,
                minor: 0,
                patch: 0,
            },
            visibility: ModuleVisibility::Private,
            reducers: Vec::new(),
            queries: Vec::new(),
            tables: Vec::new(),
            subscriptions: Vec::new(),
            type_definitions: HashMap::new(),
            authorities: Vec::new(),
            module_dependencies: Vec::new(),
            node_dependencies: Vec::new(),
        }
    }

    pub fn new(
        name: impl Into<String>,
        version: Version,
        visibility: ModuleVisibility,
        reducers: Vec<ReducerSchema>,
        queries: Vec<QuerySchema>,
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
            queries,
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

        // Helper to extract all Named type references from an IntersticeType
        fn extract_named_types(it: &IntersticeType, names: &mut Vec<String>) {
            match it {
                IntersticeType::Named(name) => names.push(name.clone()),
                IntersticeType::Vec(inner) | IntersticeType::Option(inner) => {
                    extract_named_types(inner, names);
                }
                IntersticeType::Tuple(types) => {
                    for ty in types {
                        extract_named_types(ty, names);
                    }
                }
                _ => {} // Primitive types
            }
        }

        // Recursive helper to collect a type and all its nested dependencies
        let collect_type_recursively =
            |type_name: &str,
             collected: &mut HashMap<String, IntersticeTypeDef>,
             all_types: &HashMap<String, IntersticeTypeDef>| {
                let mut stack = vec![type_name.to_string()];

                while let Some(current_name) = stack.pop() {
                    // Skip if already collected
                    if collected.contains_key(&current_name) {
                        continue;
                    }

                    // Get the type definition
                    if let Some(type_def) = all_types.get(&current_name) {
                        collected.insert(current_name.clone(), type_def.clone());

                        // Extract nested type names from this type
                        if let IntersticeTypeDef::Struct { fields, .. } = type_def {
                            for field in fields {
                                let mut nested_names = Vec::new();
                                extract_named_types(&field.field_type, &mut nested_names);
                                for nested_name in nested_names {
                                    if !collected.contains_key(&nested_name) {
                                        stack.push(nested_name);
                                    }
                                }
                            }
                        }
                    }
                }
            };

        // Collect types from public tables
        let mut tables = Vec::new();
        for table_schema in &self.tables {
            if table_schema.visibility == TableVisibility::Public {
                tables.push(table_schema.clone());

                // Collect table type and all its dependencies
                collect_type_recursively(
                    &table_schema.type_name,
                    &mut type_definitions,
                    &self.type_definitions,
                );

                // Collect field types and their dependencies
                for field in &table_schema.fields {
                    let mut nested_names = Vec::new();
                    extract_named_types(&field.field_type, &mut nested_names);
                    for type_name in nested_names {
                        collect_type_recursively(
                            &type_name,
                            &mut type_definitions,
                            &self.type_definitions,
                        );
                    }
                }
            }
        }

        // Collect types from public reducers
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

                // Collect types from reducer arguments and their dependencies
                for arg in &reducer_schema.arguments {
                    let mut nested_names = Vec::new();
                    extract_named_types(&arg.field_type, &mut nested_names);
                    for type_name in nested_names {
                        collect_type_recursively(
                            &type_name,
                            &mut type_definitions,
                            &self.type_definitions,
                        );
                    }
                }
            }
        }

        // Collect types from queries
        let queries = self.queries.clone();
        for query_schema in &queries {
            // Collect types from query arguments and their dependencies
            for arg in &query_schema.arguments {
                let mut nested_names = Vec::new();
                extract_named_types(&arg.field_type, &mut nested_names);
                for type_name in nested_names {
                    collect_type_recursively(
                        &type_name,
                        &mut type_definitions,
                        &self.type_definitions,
                    );
                }
            }

            // Collect type from query return type and its dependencies
            let mut nested_names = Vec::new();
            extract_named_types(&query_schema.return_type, &mut nested_names);
            for type_name in nested_names {
                collect_type_recursively(&type_name, &mut type_definitions, &self.type_definitions);
            }
        }

        Self {
            abi_version: self.abi_version,
            name: self.name,
            visibility: self.visibility,
            version: self.version,
            reducers,
            queries,
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
