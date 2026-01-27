use std::collections::HashMap;

use interstice_abi::{
    Row, interstice_type_def::IntersticeTypeDef, schema::TableSchema, validate_value,
};

pub struct Table {
    pub schema: TableSchema,
    pub rows: Vec<Row>,
}

pub fn validate_row(
    row: &Row,
    schema: &TableSchema,
    type_definitions: &HashMap<String, IntersticeTypeDef>,
) -> bool {
    if !validate_value(
        &row.primary_key,
        &schema.primary_key.field_type,
        type_definitions,
    ) {
        return false;
    }
    if row.entries.len() != schema.fields.len() {
        return false;
    }
    for (entry, ty) in row.entries.iter().zip(schema.fields.iter()) {
        if !validate_value(entry, &ty.field_type, type_definitions) {
            return false;
        }
    }
    true
}
