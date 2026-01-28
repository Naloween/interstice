use std::{
    fs::{self, read_dir, read_to_string},
    path::Path,
};

use interstice_abi::{ModuleSchema, ReducerSchema, TableSchema};

pub fn generate_bindings() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let bindings_dir = Path::new(&manifest_dir).join("src/bindings");

    let mut generated = "// This is automatically generated interstice rust bindings".to_string();

    if let Ok(read_binding_dir) = read_dir(bindings_dir) {
        for entry in read_binding_dir {
            let path = entry.unwrap().path();
            if path.extension().and_then(|s| s.to_str()) == Some("toml") {
                let content = read_to_string(&path).unwrap();
                let schema = ModuleSchema::from_toml_string(&content).unwrap();

                generated.push_str(&get_module_code(schema));
            }
        }
    }

    let out_dir = std::env::var("OUT_DIR").unwrap();
    fs::write(format!("{out_dir}/interstice_bindings.rs"), generated)
        .expect("Couldn't write bindings");

    println!("cargo:rerun-if-changed=src/bindings");
}

fn get_module_code(module_schema: ModuleSchema) -> String {
    let mut result = String::new();
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
            + " for interstice_sdk::ReducerContext {
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
        into_row_entries += &("self.".to_string() + &entry.name + ".clone().into()");
    }
    let mut into_struct_entries = String::new();
    for entry in &table_schema.fields {
        into_struct_entries += &(entry.name.clone() + ": row_entries.next().unwrap().into(),\n");
    }
    "
pub struct "
        .to_string()
        + &table_struct_name
        + "{
    " + &table_schema.primary_key.name
        + ": "
        + &table_schema.primary_key.field_type.to_string()
        + ",
    " + &table_entries_str
        + "
}
pub struct "
        + &table_handle_struct_name
        + "{}

impl Into<interstice_sdk::Row> for "
        + &table_struct_name
        + " {
    fn into(self) -> interstice_sdk::Row{
        Row {
            primary_key: self."
        + &table_schema.primary_key.name
        + ".into(),
            entries: vec!["
        + &into_row_entries
        + "],
        }
    }
}

impl Into<"
        + &table_struct_name
        + "> for interstice_sdk::Row {
    fn into(self) -> "
        + &table_struct_name
        + "{
        let mut row_entries = self.entries.into_iter();
        "
        + &table_struct_name
        + " {
            "
        + &table_schema.primary_key.name
        + ": self.primary_key.into(), // convert IntersticeValue → PK type
            "
        + &into_struct_entries
        + "
        }
    }
}

impl " + &table_handle_struct_name
        + "{
    pub fn insert(&self, row: "
        + &table_struct_name
        + "){
        interstice_sdk::host_calls::insert_row(
            ModuleSelection::Current,
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
        + "\".to_string()).into_iter().map(|x| x.into()).collect()
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
