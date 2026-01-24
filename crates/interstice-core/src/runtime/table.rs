use interstice_abi::{
    Row,
    schema::{TableEvent, TableSchema},
    validate_value,
};
use super::index::{PrimaryKeyIndex, SecondaryIndex};

pub struct Table {
    pub schema: TableSchema,
    pub rows: Vec<Row>,
    // Primary key index for O(log n) lookups
    pk_index: PrimaryKeyIndex,
    // Optional secondary indexes by column name
    secondary_indexes: std::collections::HashMap<String, SecondaryIndex>,
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

impl Table {
    /// Create a new table with schema
    pub fn new(schema: TableSchema) -> Self {
        Table {
            schema,
            rows: Vec::new(),
            pk_index: PrimaryKeyIndex::new(),
            secondary_indexes: std::collections::HashMap::new(),
        }
    }

    /// Insert a row into the table, maintaining index
    pub fn insert(&mut self, row: Row) -> Result<usize, String> {
        if !validate_row(&row, &self.schema) {
            return Err("Invalid row".to_string());
        }
        
        // Use primary key value as bytes for simple indexing
        let pk_bytes = format!("{:?}", row.primary_key).into_bytes();
        
        let row_index = self.rows.len();
        self.rows.push(row.clone());
        self.pk_index.insert(pk_bytes.clone(), row_index);
        
        // Update secondary indexes
        for (col_name, idx) in self.secondary_indexes.iter_mut() {
            if let Some(col_idx) = self.schema.entries.iter().position(|e| &e.name == col_name) {
                if col_idx < row.entries.len() {
                    let val_bytes = format!("{:?}", row.entries[col_idx]).into_bytes();
                    idx.insert(val_bytes, row_index);
                }
            }
        }
        
        Ok(row_index)
    }

    /// Find row by primary key
    pub fn find_by_pk(&self, pk: &interstice_abi::IntersticeValue) -> Option<(usize, &Row)> {
        let pk_bytes = format!("{:?}", pk).into_bytes();
        self.pk_index.get(&pk_bytes).map(|idx| (idx, &self.rows[idx]))
    }

    /// Query rows by secondary index
    pub fn query_by_index(&self, index_name: &str, value: &interstice_abi::IntersticeValue) -> Vec<&Row> {
        if let Some(idx) = self.secondary_indexes.get(index_name) {
            let val_bytes = format!("{:?}", value).into_bytes();
            idx.query(&val_bytes)
                .into_iter()
                .map(|row_idx| &self.rows[row_idx])
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Create a secondary index on a column
    pub fn create_index(&mut self, column_name: &str) -> Result<(), String> {
        if self.secondary_indexes.contains_key(column_name) {
            return Err(format!("Index on {} already exists", column_name));
        }
        
        if !self.schema.entries.iter().any(|e| e.name == column_name) {
            return Err(format!("Column {} not found in schema", column_name));
        }
        
        let mut idx = SecondaryIndex::new(column_name.to_string());
        
        // Populate index with existing rows
        for (row_idx, row) in self.rows.iter().enumerate() {
            if let Some(col_idx) = self.schema.entries.iter().position(|e| e.name == column_name) {
                if col_idx < row.entries.len() {
                    let val_bytes = format!("{:?}", row.entries[col_idx]).into_bytes();
                    idx.insert(val_bytes, row_idx);
                }
            }
        }
        
        self.secondary_indexes.insert(column_name.to_string(), idx);
        Ok(())
    }

    /// Get row by index (for iteration)
    pub fn get(&self, row_idx: usize) -> Option<&Row> {
        self.rows.get(row_idx)
    }

    /// Total row count
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Get all rows as iterator
    pub fn iter(&self) -> impl Iterator<Item = &Row> {
        self.rows.iter()
    }

    /// Get mutable row by index
    pub fn get_mut(&mut self, row_idx: usize) -> Option<&mut Row> {
        self.rows.get_mut(row_idx)
    }
}

