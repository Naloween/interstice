use interstice_abi::{ModuleSchema, NodeSelection};

use crate::{
    bindings::{
        query::get_query_code, reducer::get_reducer_code, table::get_table_code,
        type_definition::get_type_definition_code,
    },
    snake_to_camel_case, to_snake_case,
};

pub fn get_module_code(module_schema: ModuleSchema, node_selection: NodeSelection) -> String {
    let module_name = module_schema.name;
    let snake_module_name = to_snake_case(&module_name);
    let camel_module_name = snake_to_camel_case(&snake_module_name);
    let module_handle_name = camel_module_name.clone() + "ModuleHandle";
    let module_tables_name = camel_module_name.clone() + "Tables";
    let module_reducers_name = camel_module_name.clone() + "Reducers";
    let module_queries_name = camel_module_name.clone() + "Queries";
    let has_module_handle_trait_name = "Has".to_string() + &module_handle_name;

    let mut reducers_def_str = String::new();
    for reducer_schema in module_schema.reducers {
        reducers_def_str += &get_reducer_code(&module_name, reducer_schema, &node_selection);
    }

    let mut queries_def_str = String::new();
    for query_schema in module_schema.queries {
        queries_def_str += &get_query_code(&module_name, query_schema, &node_selection);
    }

    let mut type_definitions = String::new();
    for type_def in module_schema.type_definitions.values() {
        type_definitions += &get_type_definition_code(type_def);
    }

    let struct_handle_reducer_module_name = match &node_selection {
        NodeSelection::Current => "interstice_sdk::ReducerContext".to_string(),
        NodeSelection::Other(node_name) => snake_to_camel_case(&to_snake_case(&node_name)),
    };

    let mut module_content = String::new();

    // For node modules, wrap in an inner module. For local modules, don't wrap
    // (get_current_node_code already wraps them)
    let needs_inner_module = matches!(node_selection, NodeSelection::Other(_));

    if needs_inner_module {
        module_content += &("pub mod ".to_string() + &snake_module_name + " {\n");
    }

    let indent = if needs_inner_module { "    " } else { "" };

    // Add type definitions with appropriate indentation
    for line in type_definitions.lines() {
        module_content += indent;
        module_content += line;
        module_content += "\n";
    }

    module_content += &format!(
        "\n{}pub struct {}\n{}{{\n{}    pub tables: {},\n{}    pub reducers: {},\n{}    pub queries: {},\n{}}}\n",
        indent,
        module_handle_name,
        indent,
        indent,
        module_tables_name,
        indent,
        module_reducers_name,
        indent,
        module_queries_name,
        indent
    );

    module_content += &format!("{}pub struct {}{{}}\n", indent, module_tables_name);
    module_content += &format!("{}pub struct {}{{}}\n", indent, module_reducers_name);
    module_content += &format!("{}pub struct {}{{}}\n", indent, module_queries_name);
    module_content += &format!("{}impl {}{{\n", indent, module_reducers_name);

    // Indent reducers
    for line in reducers_def_str.lines() {
        module_content += indent;
        module_content += "    ";
        module_content += line;
        module_content += "\n";
    }

    module_content += indent;
    module_content += "}\n";
    module_content += &format!("{}impl {}{{\n", indent, module_queries_name);

    // Indent queries
    for line in queries_def_str.lines() {
        module_content += indent;
        module_content += "    ";
        module_content += line;
        module_content += "\n";
    }

    module_content += indent;
    module_content += "}\n\n";

    // Indent tables
    for table in module_schema.tables {
        let table_code = get_table_code(table, &module_tables_name);
        for line in table_code.lines() {
            module_content += indent;
            module_content += line;
            module_content += "\n";
        }
    }

    if needs_inner_module {
        module_content += "}\n\n";
    }

    // Trait paths depend on whether we have inner module
    let module_path_prefix = if needs_inner_module {
        format!("{}::", snake_module_name)
    } else {
        String::new()
    };

    module_content += &("pub trait ".to_string()
        + &has_module_handle_trait_name
        + " {\n    fn "
        + &snake_module_name
        + "(&self) -> "
        + &module_path_prefix
        + &module_handle_name
        + ";\n}\n\nimpl "
        + &has_module_handle_trait_name
        + " for "
        + &struct_handle_reducer_module_name
        + " {\n    fn "
        + &snake_module_name
        + "(&self) -> "
        + &module_path_prefix
        + &module_handle_name
        + " {\n        return "
        + &module_path_prefix
        + &module_handle_name
        + " {\n                tables: "
        + &module_path_prefix
        + &module_tables_name
        + "{},\n                reducers: "
        + &module_path_prefix
        + &module_reducers_name
        + "{},\n                queries: "
        + &module_path_prefix
        + &module_queries_name
        + "{},\n        }\n    }\n}\n\n");

    module_content
}
