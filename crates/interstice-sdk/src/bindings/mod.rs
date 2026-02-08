mod module;
mod node;
mod query;
mod reducer;
mod table;
mod type_definition;

use interstice_abi::{ModuleSchema, NodeSchema};
use node::{get_current_node_code, get_node_code};
use std::{
    fs::{self, read_dir, read_to_string},
    path::Path,
};

pub fn generate_bindings() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let bindings_dir = Path::new(&manifest_dir).join("src/bindings");

    let mut generated =
        "// This is automatically generated interstice rust bindings\n\n".to_string();
    let mut module_dependencies_code_str =
        "pub fn __GET_INTERSTICE_MODULE_DEPENDENCIES() -> Vec<interstice_sdk::ModuleDependency>{\n   vec![\n"
            .to_string();
    let mut node_dependencies_code_str =
        "pub fn __GET_INTERSTICE_NODE_DEPENDENCIES() -> Vec<interstice_sdk::NodeDependency>{\n   vec![\n"
            .to_string();
    if let Ok(read_binding_dir) = read_dir(bindings_dir) {
        let mut modules_schema = Vec::new();
        for entry in read_binding_dir {
            let path = entry.unwrap().path();
            if path.extension().and_then(|s| s.to_str()) == Some("toml") {
                let content = read_to_string(&path).unwrap();
                match ModuleSchema::from_toml_string(&content) {
                    Ok(module_schema) => {
                        module_dependencies_code_str.push_str(
                            &("        interstice_sdk::ModuleDependency{module_name: \""
                                .to_string()
                                + &module_schema.name
                                + "\".to_string(), version: interstice_sdk::Version{major:"
                                + &module_schema.version.major.to_string()
                                + ", minor:"
                                + &module_schema.version.minor.to_string()
                                + ", patch:"
                                + &module_schema.version.patch.to_string()
                                + "}},\n"),
                        );
                        modules_schema.push(module_schema);
                    }
                    Err(mod_err) => match NodeSchema::from_toml_string(&content) {
                        Ok(node_schema) => {
                            node_dependencies_code_str.push_str(
                                &("        interstice_sdk::NodeDependency{name: \"".to_string()
                                    + &node_schema.name
                                    + "\".to_string(), address: \""
                                    + &node_schema.address
                                    + "\".to_string()},\n"),
                            );
                            generated.push_str(&get_node_code(node_schema));
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

        generated.push_str(&get_current_node_code(modules_schema));
    }
    module_dependencies_code_str.push_str("    ]\n}");
    node_dependencies_code_str.push_str("    ]\n}");
    generated.push_str(&module_dependencies_code_str);
    generated.push_str(&node_dependencies_code_str);

    let out_dir = std::env::var("OUT_DIR").unwrap();
    fs::write(format!("{out_dir}/interstice_bindings.rs"), generated)
        .expect("Couldn't write bindings");

    println!("cargo:rerun-if-changed=src/bindings");
}
