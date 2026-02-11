use interstice_abi::{NodeSelection, QuerySchema};

pub fn get_query_code(
    module_name: &String,
    query_schema: QuerySchema,
    node_selection: &NodeSelection,
) -> String {
    let mut arguments_str = String::new();
    let mut arguments_values_str = String::new();
    for arg in query_schema.arguments {
        arguments_str += &(arg.name.clone() + ": " + &arg.field_type.to_string());
        arguments_values_str += &(arg.name.clone() + ".into()");
    }
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
