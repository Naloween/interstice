use interstice_abi::{NodeSelection, QuerySchema};

pub fn get_query_code(
    module_name: &String,
    query_schema: QuerySchema,
    node_selection: &NodeSelection,
) -> String {
    let arguments: Vec<String> = query_schema
        .arguments
        .iter()
        .map(|arg| arg.name.clone() + ": " + &arg.field_type.to_string())
        .collect();
    let arguments_str = arguments.join(", ");
    let arguments_values: Vec<String> = query_schema
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

    let return_type = query_schema.return_type.to_string();

    "\n    pub fn ".to_string()
        + &query_schema.name
        + "(&self, "
        + &arguments_str
        + ") -> Result<"
        + &return_type
        + ", String>{
        let res = interstice_sdk::host_calls::call_query(
            "
        + &node_selection
        + ",
            interstice_sdk::ModuleSelection::Other(\""
        + module_name
        + "\".into()),
            \""
        + &query_schema.name
        + "\".to_string(),
            interstice_sdk::IntersticeValue::Vec(vec!["
        + &arguments_values_str
        + "]),
        )?;
        Ok(res.try_into().unwrap())
    }
"
}
