use interstice_abi::{ModuleSchema, NodeSelection};

use crate::bindings::{
    query::get_query_code, reducer::get_reducer_code, table::get_table_code,
    type_definition::get_type_definition_code,
};

pub fn get_module_code(module_schema: ModuleSchema, node_selection: NodeSelection) -> String {
    let module_name = module_schema.name;
    let upped_module_name = module_name
        .chars()
        .nth(0)
        .unwrap()
        .to_uppercase()
        .to_string()
        + &module_name[1..module_name.len()];
    let module_context_name = upped_module_name.clone() + "Context";
    let module_query_context_name = upped_module_name.clone() + "QueryContext";
    let module_tables_name = upped_module_name.clone() + "Tables";
    let module_reducers_name = upped_module_name.clone() + "Reducers";
    let module_queries_name = upped_module_name.clone() + "Queries";
    let has_module_handle_trait_name = "Has".to_string() + &module_context_name;

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
        NodeSelection::Other(node_name) => node_name.clone(),
    };

    let mut result = String::new();

    result += &type_definitions;

    result += &("\npub struct ".to_string()
        + &module_context_name
        + "\n{\n    pub tables: "
        + &module_tables_name
        + ",\n    pub reducers: "
        + &module_reducers_name
        + ",\n    pub queries: "
        + &module_queries_name
        + ",\n}\npub struct "
        + &module_query_context_name
        + "\n{\n    pub tables: "
        + &module_tables_name
        + ",\n    pub queries: "
        + &module_queries_name
        + ",\n}\npub struct "
        + &module_tables_name
        + "{}\npub struct "
        + &module_reducers_name
        + "{}\npub struct "
        + &module_queries_name
        + "{}\nimpl "
        + &module_reducers_name
        + "{\n"
        + &reducers_def_str
        + "\n}\nimpl "
        + &module_queries_name
        + "{\n"
        + &queries_def_str
        + "\n}\npub trait "
        + &has_module_handle_trait_name
        + " {\n    fn "
        + &module_name
        + "(&self) -> "
        + &module_context_name
        + ";\n}\n\nimpl "
        + &has_module_handle_trait_name
        + " for "
        + &struct_handle_reducer_module_name
        + " {\n    fn "
        + &module_name
        + "(&self) -> "
        + &module_context_name
        + " {\n        return "
        + &module_context_name
        + " {\n                tables: "
        + &module_tables_name
        + "{},\n reducers: "
        + &module_reducers_name
        + "{},\n queries: "
        + &module_queries_name
        + "{},\n}\n    }\n}\n\n");

    for table in module_schema.tables {
        result += &get_table_code(table, &module_tables_name);
    }

    return result;
}
