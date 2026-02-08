use interstice_abi::{ModuleSchema, NodeSchema, NodeSelection};

use crate::bindings::module::get_module_code;

pub fn get_node_code(node_schema: NodeSchema) -> String {
    let mut result = String::new();
    let node_name = node_schema.name;
    let node_handle_name = node_name.trim().to_lowercase();

    result += &("pub struct ".to_string() + &node_name + "{}\n\n");

    let trait_handle_name = "Has".to_string() + &node_name + "Handle";

    result += &("pub trait ".to_string()
        + &trait_handle_name
        + " {\n    fn "
        + &node_handle_name
        + "(&self) -> "
        + &node_name
        + ";\n}\n\nimpl "
        + &trait_handle_name
        + " for interstice_sdk::ReducerContext{\n    fn "
        + &node_handle_name
        + "(&self) -> "
        + &node_name
        + "{\n        return "
        + &node_name
        + "{};\n    }\n}\n\n");

    for module_schema in node_schema.modules {
        result += &get_module_code(module_schema, NodeSelection::Other(node_name.clone()));
    }

    return result;
}

pub fn get_current_node_code(module_schemas: Vec<ModuleSchema>) -> String {
    let mut result = String::new();
    for module_schema in module_schemas {
        result += &get_module_code(module_schema, NodeSelection::Current);
    }
    return result;
}
