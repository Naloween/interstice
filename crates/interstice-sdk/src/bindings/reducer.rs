use interstice_abi::{NodeSelection, ReducerSchema};

pub fn get_reducer_code(
    module_name: &String,
    reducer_schema: ReducerSchema,
    node_selection: &NodeSelection,
) -> String {
    let arguments: Vec<String> = reducer_schema
        .arguments
        .iter()
        .map(|arg| arg.name.clone() + ": " + &arg.field_type.to_string())
        .collect();
    let arguments_str = arguments.join(", ");
    let arguments_values: Vec<String> = reducer_schema
        .arguments
        .iter()
        .map(|arg| arg.name.clone() + ".into()")
        .collect();
    let arguments_values_str = arguments_values.join(", ");
    let node_selection = match node_selection {
        NodeSelection::Current => "interstice_sdk::NodeSelection::Current".to_string(),
        NodeSelection::Other(node_name) => {
            "interstice_sdk::NodeSelection::Other(\"".to_string() + &node_name + "\".to_string())"
        }
    };

    "\n    pub fn ".to_string()
        + &reducer_schema.name
        + "(&self, "
        + &arguments_str
        + ") -> Result<(), String>{
        interstice_sdk::host_calls::call_reducer(
            "
        + &node_selection
        + ",
            interstice_sdk::ModuleSelection::Other(\""
        + module_name
        + "\".into()),
            \""
        + &reducer_schema.name
        + "\".to_string(),
            interstice_sdk::IntersticeValue::Vec(vec!["
        + &arguments_values_str
        + "]),
        )
    }
"
}
