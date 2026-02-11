use interstice_abi::TableSchema;

pub fn get_table_code(table_schema: TableSchema, module_tables_name: &str) -> String {
    let table_name = table_schema.name;
    let table_struct_name = table_schema.type_name;
    let table_handle_struct_name = table_struct_name.clone() + "Handle";
    let has_table_handle_trait_name = "Has".to_string() + &table_struct_name + "Handle";

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
        + "{}\n\nimpl Into<interstice_sdk::Row> for "
        + &table_struct_name
        + " {\n    fn into(self) -> interstice_sdk::Row{\n        interstice_sdk::Row {\n            primary_key: self."
        + &table_schema.primary_key.name
        + ".into(),\n            entries: vec!["
        + &into_row_entries
        + "],\n        }\n    }\n}\n\nimpl TryFrom<interstice_sdk::Row> for "
        + &table_struct_name
        + " {\n    type Error = interstice_sdk::interstice_abi::IntersticeAbiError;\n    fn try_from(row: interstice_sdk::Row) -> Result<Self, Self::Error> {\n        let mut row_entries = row.entries.into_iter();\n        Ok(Self {\n            "
        + &table_schema.primary_key.name
        + ": row.primary_key.try_into()?,\n            "
        + &into_struct_entries
        + "\n})\n    }\n}\n\nimpl "
        + &table_handle_struct_name
        + "{\n    pub fn insert(&self, row: "
        + &table_struct_name
        + ") -> Result<"
        + &table_struct_name
        + ", String>{\n        interstice_sdk::host_calls::insert_row(\n           interstice_sdk:: ModuleSelection::Current,\n            \""
        + &table_name
        + "\".to_string(),\n            row.into(),\n        )\n        .map(|row| row.try_into().unwrap())\n    }\n\n    pub fn scan(&self) -> Result<Vec<"
        + &table_struct_name
        + ">, String>{\n        interstice_sdk::host_calls::scan(interstice_sdk::ModuleSelection::Current, \""
        + &table_name
        + "\".to_string()).map(|rows| rows.into_iter().map(|x| x.try_into().unwrap()).collect())\n    }\n}\n\n"
        + "pub trait "
        + &has_table_handle_trait_name
        + " {\n    fn "
        + &table_name
        + "(&self) -> "
        + &table_handle_struct_name
        + ";\n}\n\n"
        + "impl "
        + &has_table_handle_trait_name
        + " for "
        + &module_tables_name
        + " {\n    fn "
        + &table_name
        + "(&self) -> "
        + &table_handle_struct_name
        + " {\n        return "
        + &table_handle_struct_name
        + " {}\n    }\n}\n"
}
