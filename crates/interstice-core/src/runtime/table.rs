use interstice_abi::{IndexKey, Row, TableSchema};
use wgpu::naga::FastHashMap;

use crate::IntersticeError;

pub struct Table {
    pub schema: TableSchema,
    rows: Vec<Row>,
    primary_key_index: FastHashMap<IndexKey, usize>,
}

impl Table {
    pub fn new(schema: TableSchema) -> Self {
        Self {
            schema,
            rows: Vec::new(),
            primary_key_index: FastHashMap::default(),
        }
    }

    pub fn insert(&mut self, row: Row) -> Result<(), IntersticeError> {
        let index = self.rows.len();
        self.primary_key_index.insert(
            row.primary_key
                .clone()
                .try_into()
                .map_err(|err| IntersticeError::Internal(err))?,
            index,
        );
        self.rows.push(row);
        Ok(())
    }

    pub fn update(&mut self, row: Row) -> Result<Row, IntersticeError> {
        let primary_key_value: IndexKey = row
            .primary_key
            .clone()
            .try_into()
            .map_err(|err| IntersticeError::Internal(err))?;
        if let Some(&index) = self.primary_key_index.get(&primary_key_value) {
            let old_row = std::mem::replace(&mut self.rows[index], row);
            Ok(old_row)
        } else {
            Err(IntersticeError::RowNotFound { primary_key_value })
        }
    }

    pub fn delete(&mut self, primary_key_value: &IndexKey) -> Result<Row, IntersticeError> {
        if let Some(&index) = self.primary_key_index.get(primary_key_value) {
            let deleted_row = self.rows.swap_remove(index);
            self.primary_key_index.remove(primary_key_value);
            // Update the swapped row's index in the primary key index
            if index < self.rows.len() {
                let swapped_row = &self.rows[index];
                let swapped_primary_key_value: IndexKey = swapped_row
                    .primary_key
                    .clone()
                    .try_into()
                    .map_err(|err| IntersticeError::Internal(err))?;
                self.primary_key_index
                    .insert(swapped_primary_key_value, index);
            }
            Ok(deleted_row)
        } else {
            Err(IntersticeError::RowNotFound {
                primary_key_value: primary_key_value.clone(),
            })
        }
    }

    pub fn get_by_primary_key(&self, primary_key_value: &IndexKey) -> Option<&Row> {
        self.primary_key_index
            .get(primary_key_value)
            .map(|&index| &self.rows[index])
    }

    pub fn scan(&self) -> &[Row] {
        &self.rows
    }
}
