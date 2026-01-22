use interstice_abi::{Row, schema::TableSchema, validate_value};

pub struct Table {
    pub schema: TableSchema,
    pub rows: Vec<Row>,
}

pub fn validate_row(row: &Row, schema: &TableSchema) -> bool {
    if !validate_value(&row.primary_key, &schema.primary_key.value_type) {
        return false;
    }
    if row.entries.len() != schema.entries.len() {
        return false;
    }
    for (entry, ty) in row.entries.iter().zip(schema.entries.iter()) {
        if !validate_value(entry, &ty.value_type) {
            return false;
        }
    }
    true
}
