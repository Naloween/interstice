use std::{
    fs::{self, read_dir, read_to_string},
    path::Path,
};

use interstice_abi::{
    IntersticeTypeDef, IntersticeValue, ModuleSchema, NodeSchema, NodeSelection, ReducerSchema,
    TableSchema,
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
                    Err(err) => {
                        if let Ok(node_schema) = NodeSchema::from_toml_string(&content) {
                            node_dependencies_code_str.push_str(
                                &("        interstice_sdk::NodeDependency{name: \"".to_string()
                                    + &node_schema.name
                                    + "\".to_string(), adress: \""
                                    + &node_schema.adress
                                    + "\".to_string()},\n"),
                            );
                            generated.push_str(&get_node_code(node_schema));
                        } else {
                            println!(
                                "cargo:warning=Skipped toml file because of error: {}",
                                err.message()
                            );
                        }
                    }
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

fn get_node_code(node_schema: NodeSchema) -> String {
    let mut result = String::new();
    let node_name = node_schema.name;

    result += "pub struct NodeName{}\n";

    for module_schema in node_schema.modules {
        result += &get_module_code(module_schema, NodeSelection::Other(node_name.clone()));
    }

    return result;
}

fn get_current_node_code(module_schemas: Vec<ModuleSchema>) -> String {
    let mut result = String::new();
    for module_schema in module_schemas {
        result += &get_module_code(module_schema, NodeSelection::Current);
    }
    return result;
}

fn get_module_code(module_schema: ModuleSchema, node_selection: NodeSelection) -> String {
    let module_name = module_schema.name;
    let upped_module_name = module_name
        .chars()
        .nth(0)
        .unwrap()
        .to_uppercase()
        .to_string()
        + &module_name[1..module_name.len()];
    let module_context_name = upped_module_name.clone() + "Context";
    let module_tables_name = upped_module_name.clone() + "Tables";
    let module_reducers_name = upped_module_name.clone() + "Reducers";
    let has_module_handle_trait_name = "Has".to_string() + &module_context_name;

    let mut reducers_def_str = String::new();
    for reducer_schema in module_schema.reducers {
        reducers_def_str += &get_reducer_code(&module_name, reducer_schema);
    }

    let mut type_definitions = String::new();
    for type_def in module_schema.type_definitions.values() {
        // if module_schema
        //     .tables
        //     .iter()
        //     .find(|t| t.name == type_def.get_name().to_lowercase())
        //     .is_none()
        // {
        // }
        type_definitions += &get_type_definition_code(type_def);
    }

    let struct_handle_module_name = match node_selection {
        NodeSelection::Current => "interstice_sdk::ReducerContext".to_string(),
        NodeSelection::Other(node_name) => {
            let upped_node_name = node_name.chars().nth(0).unwrap().to_uppercase().to_string()
                + &node_name[1..node_name.len()];
            upped_node_name
        }
    };

    let mut result = String::new();

    result += &type_definitions;

    result +=
        &("
pub struct "
            .to_string()
            + &module_context_name
            + "
{
    pub tables: "
            + &module_tables_name
            + ",
    pub reducers: "
            + &module_reducers_name
            + ",
}
pub struct " + &module_tables_name
            + "{}
pub struct " + &module_reducers_name
            + "{}
impl " + &module_reducers_name
            + "{
" + &reducers_def_str
            + "
}
pub trait " + &has_module_handle_trait_name
            + " {
    fn " + &module_name
            + "(&self) -> "
            + &module_context_name
            + ";
}

impl " + &has_module_handle_trait_name
            + " for "
            + &struct_handle_module_name
            + " {
    fn " + &module_name
            + "(&self) -> "
            + &module_context_name
            + " {
        return "
            + &module_context_name
            + " {
                tables: "
            + &module_tables_name
            + "{},\n reducers: "
            + &module_reducers_name
            + "{},\n}
    }
}
");

    for table in module_schema.tables {
        result += &get_table_code(table, &module_tables_name);
    }

    return result;
}

fn get_table_code(table_schema: TableSchema, module_tables_name: &str) -> String {
    let table_name = table_schema.name;
    let table_struct_name = table_name
        .chars()
        .nth(0)
        .unwrap()
        .to_uppercase()
        .to_string()
        + &table_name[1..table_name.len()];
    let table_handle_struct_name = table_struct_name.clone() + "Handle";
    let has_table_handle_trait_name = "Has".to_string() + &table_struct_name + "Handle";

    let mut table_entries_str = String::new();
    for entry in &table_schema.fields {
        table_entries_str += &(entry.name.clone() + ": " + &entry.field_type.to_string() + ",\n");
    }
    let mut into_row_entries = String::new();
    for entry in &table_schema.fields {
        into_row_entries += &("self.".to_string() + &entry.name + ".into(), ");
    }
    let mut into_struct_entries = String::new();
    for entry in &table_schema.fields {
        into_struct_entries +=
            &(entry.name.clone() + ": row_entries.next().unwrap().try_into()?,\n");
    }
    "pub struct ".to_string()
        + &table_handle_struct_name
        + "{}

impl Into<interstice_sdk::Row> for "
        + &table_struct_name
        + " {
    fn into(self) -> interstice_sdk::Row{
        interstice_sdk::Row {
            primary_key: self."
        + &table_schema.primary_key.name
        + ".into(),
            entries: vec!["
        + &into_row_entries
        + "],
        }
    }
}

impl TryFrom<interstice_sdk::Row> for "
        + &table_struct_name
        + " {
    type Error = interstice_sdk::interstice_abi::IntersticeAbiError;
    fn try_from(row: interstice_sdk::Row) -> Result<Self, Self::Error> {
        let mut row_entries = row.entries.into_iter();
        Ok(Self {
            "
        + &table_schema.primary_key.name
        + ": row.primary_key.try_into()?,
            "
        + &into_struct_entries
        + "
})
    }
}

impl " + &table_handle_struct_name
        + "{
    pub fn insert(&self, row: "
        + &table_struct_name
        + "){
        interstice_sdk::host_calls::insert_row(
           interstice_sdk:: ModuleSelection::Current,
            \""
        + &table_name
        + "\".to_string(),
            row.into(),
        );
    }

    pub fn scan(&self) -> Vec<"
        + &table_struct_name
        + ">{
        interstice_sdk::host_calls::scan(interstice_sdk::ModuleSelection::Current, \""
        + &table_name
        + "\".to_string()).into_iter().map(|x| x.try_into().unwrap()).collect()
    }
}

pub trait "
        + &has_table_handle_trait_name
        + " {
    fn " + &table_name
        + "(&self) -> "
        + &table_handle_struct_name
        + ";
}

impl " + &has_table_handle_trait_name
        + " for "
        + &module_tables_name
        + " {
    fn " + &table_name
        + "(&self) -> "
        + &table_handle_struct_name
        + " {
        return "
        + &table_handle_struct_name
        + " {}
    }
}
"
}

fn get_reducer_code(module_name: &String, reducer_schema: ReducerSchema) -> String {
    let mut arguments_str = String::new();
    let mut arguments_values_str = String::new();
    for arg in reducer_schema.arguments {
        arguments_str += &(arg.name.clone() + ": " + &arg.field_type.to_string());
        arguments_values_str += &(arg.name.clone() + ".into()");
    }
    "
    pub fn "
        .to_string()
        + &reducer_schema.name
        + "(&self, "
        + &arguments_str
        + "){
        interstice_sdk::host_calls::call_reducer(
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

fn get_type_definition_code(type_def: &IntersticeTypeDef) -> String {
    match type_def {
        IntersticeTypeDef::Struct { name, fields } => {
            let mut result =
                "#[derive(interstice_sdk::interstice_abi_macros::IntersticeType)]\npub struct "
                    .to_string()
                    + &name
                    + "{\n";
            for field in fields {
                result += &("   pub ".to_string()
                    + &field.name
                    + ": "
                    + &field.field_type.to_string()
                    + ",\n");
            }
            result += "}\n";
            return result;
        }
        IntersticeTypeDef::Enum { name, variants } => {
            let mut result =
                "#[derive(interstice_sdk::interstice_abi_macros::IntersticeType)]\npub enum "
                    .to_string()
                    + &name
                    + "{\n";
            for variant in variants {
                match &variant.field_type {
                    interstice_abi::IntersticeType::Void => {
                        result += &(variant.name.clone() + ",\n");
                    }
                    interstice_abi::IntersticeType::Tuple(interstice_types) => {
                        let mut inners = String::new();
                        for field_type in interstice_types {
                            inners += &(field_type.to_string() + ", ");
                        }

                        result += &(variant.name.clone() + "(" + &inners + ")" + ",\n");
                    }
                    field_type => {
                        result +=
                            &(variant.name.clone() + "(" + &field_type.to_string() + ")" + ",\n");
                    }
                }
            }
            result += "}\n";
            return result;
        }
    };
}
