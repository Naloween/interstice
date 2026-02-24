use interstice_abi::{ModuleSchema, NodeSchema, NodeSelection};

use crate::{bindings::module::get_module_code_parts, to_camel_case, to_snake_case};

pub fn get_node_code(node_schema: NodeSchema) -> String {
    let mut result = String::new();
    let node_name = to_snake_case(&node_schema.name);
    let node_type_str = to_camel_case(&node_name);
    let original_node_name = node_schema.name.clone();
    let mut all_traits = String::new();

    // Start node module
    result += &format!("pub mod {} {{\n", node_name);
    result += &("    pub struct ".to_string() + &node_type_str + "{}\n\n");

    for module_schema in node_schema.modules {
        let module_output = get_module_code_parts(
            module_schema,
            NodeSelection::Other(original_node_name.clone()),
        );
        // Indent the module content
        for line in module_output.module_content.lines() {
            result += "    ";
            result += line;
            result += "\n";
        }
        // Collect traits to add after the module
        all_traits += &module_output.trait_definition;
    }

    // End node module
    result += "}\n\n";

    // Add top-level traits
    result += &all_traits;

    // Trait at top level for node handle access
    let trait_handle_name = "Has".to_string() + &node_type_str + "Handle";
    result += &("pub trait ".to_string()
        + &trait_handle_name
        + " {\n    fn "
        + &node_name
        + "(&self) -> "
        + &node_name
        + "::"
        + &node_type_str
        + ";\n}\n\nimpl "
        + &trait_handle_name
        + " for interstice_sdk::ReducerContext{\n    fn "
        + &node_name
        + "(&self) -> "
        + &node_name
        + "::"
        + &node_type_str
        + "{\n        return "
        + &node_name
        + "::"
        + &node_type_str
        + "{};\n    }\n}\n\n");

    return result;
}

pub fn get_current_node_code(module_schemas: Vec<ModuleSchema>) -> String {
    let mut result = String::new();
    let mut all_traits = String::new();

    for module_schema in module_schemas {
        let module_name = crate::to_snake_case(&module_schema.name);
        // Start module
        result += &format!("pub mod {} {{\n", module_name);

        let module_output = get_module_code_parts(module_schema, NodeSelection::Current);
        // Indent the module content
        for line in module_output.module_content.lines() {
            result += "    ";
            result += line;
            result += "\n";
        }

        // End module
        result += "}\n\n";

        // Collect traits to add after all modules
        all_traits += &module_output.trait_definition;
    }

    // Add all top-level traits
    result += &all_traits;

    return result;
}
