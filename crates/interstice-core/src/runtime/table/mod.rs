mod auto_inc;
mod index;

pub(crate) use auto_inc::{AutoIncState, TableAutoIncSnapshot};

use crate::IntersticeError;
use index::*;
use interstice_abi::{IndexKey, IndexQuery, Row, TableSchema};
use wgpu::naga::FastHashMap;

pub struct Table {
    pub schema: TableSchema,
    rows: Vec<Row>,
    primary_key_index: FastHashMap<IndexKey, usize>,
    indexes: Vec<TableIndex>,
    primary_key_auto_inc: bool,
    primary_key_auto_inc_state: Option<AutoIncState>,
}

impl Table {
    pub fn new(schema: TableSchema) -> Self {
        let field_positions = schema
            .fields
            .iter()
            .enumerate()
            .map(|(index, field)| (field.name.clone(), index))
            .collect::<FastHashMap<_, _>>();

        let indexes = schema
            .indexes
            .iter()
            .filter_map(|index_schema| {
                field_positions
                    .get(&index_schema.field_name)
                    .and_then(|&field_index| {
                        schema.fields.get(field_index).map(|field_def| {
                            TableIndex::new(index_schema, field_index, &field_def.field_type)
                        })
                    })
            })
            .collect();
        let primary_key_auto_inc = schema.primary_key_auto_inc;
        let primary_key_auto_inc_state = if primary_key_auto_inc {
            AutoIncState::from_type(&schema.primary_key.field_type)
        } else {
            None
        };

        Self {
            schema,
            rows: Vec::new(),
            primary_key_index: FastHashMap::default(),
            indexes,
            primary_key_auto_inc,
            primary_key_auto_inc_state,
        }
    }

    pub fn validate_insert(&self, row: &Row) -> Result<(), IntersticeError> {
        let primary_key_value: IndexKey = row
            .primary_key
            .clone()
            .try_into()
            .map_err(|err| IntersticeError::Internal(err))?;

        if self.primary_key_index.contains_key(&primary_key_value) {
            return Err(IntersticeError::UniqueConstraintViolation {
                table_name: self.schema.name.clone(),
                field_name: self.schema.primary_key.name.clone(),
            });
        }

        for table_index in &self.indexes {
            if table_index.unique {
                let key = table_index.key_from_row(row)?;
                if table_index.contains_key(&key) {
                    return Err(IntersticeError::UniqueConstraintViolation {
                        table_name: self.schema.name.clone(),
                        field_name: table_index.field_name.clone(),
                    });
                }
            }
        }

        Ok(())
    }

    pub fn validate_update(&self, row: &Row) -> Result<(), IntersticeError> {
        let primary_key_value: IndexKey = row
            .primary_key
            .clone()
            .try_into()
            .map_err(|err| IntersticeError::Internal(err))?;

        let current_index = *self
            .primary_key_index
            .get(&primary_key_value)
            .ok_or(IntersticeError::RowNotFound { primary_key_value })?;

        let existing_row = self
            .rows
            .get(current_index)
            .ok_or_else(|| IntersticeError::Internal("Row index out of bounds".into()))?;

        for table_index in &self.indexes {
            let old_key = table_index.key_from_row(existing_row)?;
            let new_key = table_index.key_from_row(row)?;

            if table_index.auto_inc && old_key != new_key {
                return Err(IntersticeError::AutoIncUpdateNotAllowed {
                    table_name: self.schema.name.clone(),
                    field_name: table_index.field_name.clone(),
                });
            }

            if table_index.unique && old_key != new_key {
                if let Some(positions) = table_index.positions(&new_key) {
                    if positions.iter().any(|pos| *pos != current_index) {
                        return Err(IntersticeError::UniqueConstraintViolation {
                            table_name: self.schema.name.clone(),
                            field_name: table_index.field_name.clone(),
                        });
                    }
                }
            }
        }

        Ok(())
    }

    pub fn validate_delete(&self, primary_key_value: &IndexKey) -> Result<(), IntersticeError> {
        if self.primary_key_index.contains_key(primary_key_value) {
            Ok(())
        } else {
            Err(IntersticeError::RowNotFound {
                primary_key_value: primary_key_value.clone(),
            })
        }
    }

    pub fn insert(&mut self, row: Row) -> Result<(), IntersticeError> {
        let row = row;
        self.sync_auto_inc_from_row(&row)?;

        let index = self.rows.len();
        let primary_key_value: IndexKey = row
            .primary_key
            .clone()
            .try_into()
            .map_err(|err| IntersticeError::Internal(err))?;

        if self.primary_key_index.contains_key(&primary_key_value) {
            return Err(IntersticeError::UniqueConstraintViolation {
                table_name: self.schema.name.clone(),
                field_name: self.schema.primary_key.name.clone(),
            });
        }

        for table_index in &mut self.indexes {
            let key = table_index.key_from_row(&row)?;
            table_index.insert(index, key, &self.schema.name)?;
        }

        self.primary_key_index.insert(primary_key_value, index);
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
            for table_index in &mut self.indexes {
                let old_key = table_index.key_from_row(&old_row)?;
                let new_key = table_index.key_from_row(&self.rows[index])?;
                table_index.update(index, old_key, new_key, &self.schema.name)?;
            }
            Ok(old_row)
        } else {
            Err(IntersticeError::RowNotFound { primary_key_value })
        }
    }

    pub fn delete(&mut self, primary_key_value: &IndexKey) -> Result<Row, IntersticeError> {
        if let Some(&index) = self.primary_key_index.get(primary_key_value) {
            let deleted_row = self.rows.swap_remove(index);
            self.primary_key_index.remove(primary_key_value);

            for table_index in &mut self.indexes {
                let deleted_key = table_index.key_from_row(&deleted_row)?;
                table_index.remove(index, deleted_key);
            }

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

                let swapped_position = self.rows.len();
                for table_index in &mut self.indexes {
                    let swapped_key = table_index.key_from_row(swapped_row)?;
                    table_index.replace_position(swapped_position, index, swapped_key);
                }
            }
            Ok(deleted_row)
        } else {
            Err(IntersticeError::RowNotFound {
                primary_key_value: primary_key_value.clone(),
            })
        }
    }

    pub fn clear(&mut self) {
        let schema = self.schema.clone();
        *self = Table::new(schema);
    }

    pub fn get_by_primary_key(&self, primary_key_value: &IndexKey) -> Option<&Row> {
        self.primary_key_index
            .get(primary_key_value)
            .map(|&index| &self.rows[index])
    }

    pub fn scan(&self) -> &[Row] {
        &self.rows
    }

    pub fn get_by_index(
        &self,
        field_name: &str,
        query: &IndexQuery,
    ) -> Result<Vec<&Row>, IntersticeError> {
        let table_index = self
            .indexes
            .iter()
            .find(|index| index.field_name == field_name)
            .ok_or_else(|| IntersticeError::IndexNotFound {
                table_name: self.schema.name.clone(),
                field_name: field_name.to_string(),
            })?;

        let positions = table_index.scan(query, &self.schema.name)?;
        Ok(positions
            .into_iter()
            .filter_map(|pos| self.rows.get(pos))
            .collect())
    }

    pub fn snapshot_rows(&self) -> Vec<Row> {
        self.rows.clone()
    }

    pub fn restore_from_rows(&mut self, rows: Vec<Row>) -> Result<(), IntersticeError> {
        let schema = self.schema.clone();
        *self = Table::new(schema);
        for row in rows {
            self.insert(row)?;
        }
        Ok(())
    }
}
