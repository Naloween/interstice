use interstice_abi::{
    Row,
    schema::{TableEvent, TableSchema},
    validate_value,
};

pub struct Table {
    pub schema: TableSchema,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone)]
pub enum TableEventInstance {
    TableInsertEvent {
        module_name: String,
        table_name: String,
        inserted_row: Row,
    },
    TableUpdateEvent {
        module_name: String,
        table_name: String,
        old_row: Row,
        new_row: Row,
    },
    TableDeleteEvent {
        module_name: String,
        table_name: String,
        deleted_row: Row,
    },
}

impl TableEventInstance {
    pub fn get_event(&self) -> TableEvent {
        match self {
            TableEventInstance::TableInsertEvent {
                module_name: _,
                table_name: _,
                inserted_row: _,
            } => TableEvent::Insert,
            TableEventInstance::TableUpdateEvent {
                module_name: _,
                table_name: _,
                old_row: _,
                new_row: _,
            } => TableEvent::Update,
            TableEventInstance::TableDeleteEvent {
                module_name: _,
                table_name: _,
                deleted_row: _,
            } => TableEvent::Delete,
        }
    }

    pub fn get_module_name(&self) -> &String {
        match self {
            TableEventInstance::TableInsertEvent {
                module_name,
                table_name: _,
                inserted_row: _,
            } => module_name,
            TableEventInstance::TableUpdateEvent {
                module_name,
                table_name: _,
                old_row: _,
                new_row: _,
            } => module_name,
            TableEventInstance::TableDeleteEvent {
                module_name,
                table_name: _,
                deleted_row: _,
            } => module_name,
        }
    }

    pub fn get_table_name(&self) -> &String {
        match self {
            TableEventInstance::TableInsertEvent {
                module_name: _,
                table_name,
                inserted_row: _,
            } => table_name,
            TableEventInstance::TableUpdateEvent {
                module_name: _,
                table_name,
                old_row: _,
                new_row: _,
            } => table_name,
            TableEventInstance::TableDeleteEvent {
                module_name: _,
                table_name,
                deleted_row: _,
            } => table_name,
        }
    }
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
