mod module;
mod node;
mod query;
mod reducer;
mod table;
mod type_definition;

use interstice_abi::{ModuleSchema, NodeSchema};
use node::{get_current_node_code, get_node_code};
use proc_macro2::TokenStream;
use quote::quote;
use std::{
    fs::{self, read_dir, read_to_string},
    path::Path,
};

pub fn generate_bindings() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let bindings_dir = Path::new(&manifest_dir).join("src/bindings");

    let mut generated_items: Vec<TokenStream> = Vec::new();
    let mut module_dependency_entries: Vec<TokenStream> = Vec::new();
    let mut node_dependency_entries: Vec<TokenStream> = Vec::new();
    let mut replicated_table_match_arms: Vec<TokenStream> = Vec::new();
    let mut known_schema_node_names: Vec<String> = Vec::new();

    if let Ok(read_binding_dir) = read_dir(bindings_dir) {
        let mut modules_schema = Vec::new();
        for entry in read_binding_dir {
            let path = entry.unwrap().path();
            if path.extension().and_then(|s| s.to_str()) == Some("toml") {
                let content = read_to_string(&path).unwrap();
                match ModuleSchema::from_toml_string(&content) {
                    Ok(module_schema) => {
                        let module_name = module_schema.name.clone();
                        let major = module_schema.version.major;
                        let minor = module_schema.version.minor;
                        let patch = module_schema.version.patch;
                        module_dependency_entries.push(quote! {
                            interstice_sdk::ModuleDependency {
                                module_name: #module_name.to_string(),
                                version: interstice_sdk::Version { major: #major, minor: #minor, patch: #patch },
                            }
                        });
                        modules_schema.push(module_schema);
                    }
                    Err(mod_err) => match NodeSchema::from_toml_string(&content) {
                        Ok(node_schema) => {
                            let node_name = node_schema.name.clone();
                            let address = node_schema.address.clone();
                            known_schema_node_names.push(node_name.clone());

                            for module in &node_schema.modules {
                                let module_name = module.name.clone();
                                for table in &module.tables {
                                    let table_name = table.name.clone();
                                    let is_public =
                                        table.visibility == interstice_abi::TableVisibility::Public;
                                    replicated_table_match_arms.push(quote! {
                                        (#node_name, #module_name, #table_name) => Some(#is_public)
                                    });
                                }
                            }

                            node_dependency_entries.push(quote! {
                                interstice_sdk::NodeDependency {
                                    name: #node_name.to_string(),
                                    address: #address.to_string(),
                                }
                            });

                            generated_items.push(
                                get_node_code(node_schema)
                                    .parse::<TokenStream>()
                                    .expect("Failed to parse generated node bindings tokens"),
                            );
                        }
                        Err(node_err) => {
                            println!(
                                "cargo:warning=Skipped toml file because of module schema error: {} and node schema error: {}",
                                mod_err.message(),
                                node_err.message()
                            );
                        }
                    },
                };
            }
        }

        generated_items.push(
            get_current_node_code(modules_schema)
                .parse::<TokenStream>()
                .expect("Failed to parse generated current-node bindings tokens"),
        );
    }

    let generated_tokens = quote! {
        #(#generated_items)*

        #[allow(non_snake_case)]
        pub fn __INTERSTICE_VALIDATE_REPLICATED_TABLE(
            node_name: &str,
            module_name: &str,
            table_name: &str,
            node_dependencies: &[interstice_sdk::NodeDependency],
        ) -> Result<(), String> {
            if !node_dependencies.iter().any(|dep| dep.name == node_name) {
                return Err(format!(
                    "Replicated table '{}.{}.{}' requires '{}' in node dependencies",
                    node_name, module_name, table_name, node_name
                ));
            }

            let table_visibility = match (node_name, module_name, table_name) {
                #(#replicated_table_match_arms,)*
                _ => None,
            };

            match table_visibility {
                Some(true) => Ok(()),
                Some(false) => Err(format!(
                    "Replicated table '{}.{}.{}' exists but is not public",
                    node_name, module_name, table_name
                )),
                None => {
                    let known_schema_nodes = vec![#(#known_schema_node_names.to_string()),*];
                    if !known_schema_nodes.iter().any(|name| name == node_name) {
                        Err(format!(
                            "Replicated table '{}.{}.{}' cannot be validated: no node schema found for '{}'. Add a node schema TOML under src/bindings",
                            node_name, module_name, table_name, node_name
                        ))
                    } else {
                        Err(format!(
                            "Replicated table '{}.{}.{}' not found in loaded node schemas",
                            node_name, module_name, table_name
                        ))
                    }
                }
            }
        }

        #[allow(non_snake_case)]
        pub fn __GET_INTERSTICE_MODULE_DEPENDENCIES() -> Vec<interstice_sdk::ModuleDependency> {
            vec![
                #(#module_dependency_entries),*
            ]
        }

        #[allow(non_snake_case)]
        pub fn __GET_INTERSTICE_NODE_DEPENDENCIES() -> Vec<interstice_sdk::NodeDependency> {
            vec![
                #(#node_dependency_entries),*
            ]
        }
    };

    let out_dir = std::env::var("OUT_DIR").unwrap();
    fs::write(
        format!("{out_dir}/interstice_bindings.rs"),
        format!(
            "// This is automatically generated interstice rust bindings\n\n{}",
            generated_tokens
        ),
    )
    .expect("Couldn't write bindings");

    println!("cargo:rerun-if-changed=src/bindings");
}
