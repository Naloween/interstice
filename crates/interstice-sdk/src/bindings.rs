use std::{
    fs::{self, read_dir, read_to_string},
    path::Path,
};

use interstice_abi::ModuleSchema;

pub fn generate_bindings() {
    println!("cargo::warning={}", "Generating bindings...");
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let bindings_dir = Path::new(&manifest_dir).join("src/bindings");

    let mut generated = "// This is automatically generated interstice rust bindings".to_string();

    if let Ok(read_binding_dir) = read_dir(bindings_dir) {
        for entry in read_binding_dir {
            let path = entry.unwrap().path();
            if path.extension().and_then(|s| s.to_str()) == Some("toml") {
                let content = read_to_string(&path).unwrap();
                let schema = ModuleSchema::from_toml_string(&content).unwrap();

                generated.push_str(&generate_rust_for_schema(schema));
            }
        }
    }

    let out_dir = std::env::var("OUT_DIR").unwrap();
    fs::write(format!("{out_dir}/interstice_bindings.rs"), generated)
        .expect("Couldn't write bindings");

    println!("cargo:rerun-if-changed=src/bindings");
}

fn generate_rust_for_schema(schema: ModuleSchema) -> String {
    let mut result = String::new();

    let module_name = schema.name;
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
    let mut reducers_def_str = String::new();
    for reducer in schema.reducers {
        let mut arguments_str = String::new();
        let mut arguments_values_str = String::new();
        for arg in reducer.arguments {
            arguments_str += &(arg.name.clone() + ": " + arg.value_type.into());
            arguments_values_str += &(arg.name.clone() + ".into()");
        }

        reducers_def_str += &("
    pub fn "
            .to_string()
            + &reducer.name
            + "(&self, "
            + &arguments_str
            + "){
        interstice_sdk::host_calls::call_reducer(
            ModuleSelection::Other(\""
            + &module_name
            + "\".into()),
            \""
            + &reducer.name
            + "\".to_string(),
            IntersticeValue::Vec(vec!["
            + &arguments_values_str
            + "]),
        );
    }
");
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
");
    let has_module_handle_trait_name = "Has".to_string() + &module_context_name;

    for table in schema.tables {
        let table_name = table.name;
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
        for entry in &table.entries {
            table_entries_str +=
                &(entry.name.clone() + ": " + entry.value_type.clone().into() + ",\n");
        }
        let mut into_row_entries = String::new();
        for entry in &table.entries {
            into_row_entries += &("self.".to_string() + &entry.name + ".clone().into()");
        }
        let mut into_struct_entries = String::new();
        for entry in &table.entries {
            into_struct_entries +=
                &(entry.name.clone() + ": row_entries.next().unwrap().into(),\n");
        }
        result +=
            &("
pub struct "
                .to_string()
                + &table_struct_name
                + "{
    " + &table.primary_key.name
                + ": "
                + table.primary_key.value_type.into()
                + ",
    " + &table_entries_str
                + "
}
pub struct " + &table_handle_struct_name
                + "{}

impl Into<interstice_sdk::Row> for "
                + &table_struct_name
                + " {
    fn into(self) -> interstice_sdk::Row{
        Row {
            primary_key: self."
                + &table.primary_key.name
                + ".into(),
            entries: vec!["
                + &into_row_entries
                + "],
        }
    }
}

impl Into<" + &table_struct_name
                + "> for interstice_sdk::Row {
    fn into(self) -> "
                + &table_struct_name
                + "{
        let mut row_entries = self.entries.into_iter();
        " + &table_struct_name
                + " {
            " + &table.primary_key.name
                + ": self.primary_key.into(), // convert IntersticeValue → PK type
            " + &into_struct_entries
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
            \"" + &table_name
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

pub trait " + &has_table_handle_trait_name
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
        return " + &table_handle_struct_name
                + " {}
    }
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
        return " + &module_context_name
                + " {
                tables: "
                + &module_tables_name
                + "{},\n reducers: "
                + &module_reducers_name
                + "{},\n}
    }
}
");
    }

    return result;
}
