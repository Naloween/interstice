use interstice_abi::{NodeSelection, ReducerSchema};

pub fn get_reducer_code(
    module_name: &String,
    reducer_schema: ReducerSchema,
    node_selection: &NodeSelection,
) -> String {
    let mut arguments_str = String::new();
    let mut arguments_values_str = String::new();
    for arg in reducer_schema.arguments {
        arguments_str += &(arg.name.clone() + ": " + &arg.field_type.to_string());
        arguments_values_str += &(arg.name.clone() + ".into()");
    }
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
        + "){
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
        );
    }
"
}
