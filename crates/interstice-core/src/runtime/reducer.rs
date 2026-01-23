use crate::runtime::Runtime;
use crate::runtime::table::TableEventInstance;
use crate::{error::IntersticeError, runtime::table::validate_row};
use interstice_abi::schema::TableEvent;
use interstice_abi::{DeleteRowRequest, InsertRowRequest, IntersticeValue, UpdateRowRequest};

#[derive(Debug)]
pub struct Transaction {
    pub inserts: Vec<InsertRowRequest>,
    pub updates: Vec<UpdateRowRequest>,
    pub deletes: Vec<DeleteRowRequest>,
}

#[derive(Debug)]
pub struct ReducerFrame {
    pub module: String,
    pub reducer: String,
    pub transaction: Transaction,
    pub emitted_events: Vec<TableEventInstance>,
}

impl ReducerFrame {
    pub fn new(module: String, reducer: String) -> Self {
        Self {
            module,
            reducer,
            transaction: Transaction {
                inserts: Vec::new(),
                updates: Vec::new(),
                deletes: Vec::new(),
            },
            emitted_events: Vec::new(),
        }
    }
}

impl Runtime {
    pub(crate) fn invoke_reducer(
        &mut self,
        module_name: &str,
        reducer_name: &str,
        args: IntersticeValue,
    ) -> Result<(IntersticeValue, Vec<TableEventInstance>), IntersticeError> {
        // Lookup module
        let module = self
            .modules
            .get_mut(module_name)
            .ok_or_else(|| IntersticeError::ModuleNotFound(module_name.into()))?;

        // Check that reducer exist in schema
        module
            .schema()
            .reducers
            .iter()
            .find(|r| r.name == reducer_name)
            .ok_or_else(|| IntersticeError::ReducerNotFound {
                module: module_name.into(),
                reducer: reducer_name.into(),
            })?;

        // Detect cycles
        if self
            .call_stack
            .iter()
            .any(|f| f.module == module_name && f.reducer == reducer_name)
        {
            return Err(IntersticeError::ReducerCycle {
                module: module_name.into(),
                reducer: reducer_name.into(),
            });
        }

        // Push frame
        self.call_stack
            .push(ReducerFrame::new(module_name.into(), reducer_name.into()));

        // Call WASM function
        let result = module.call_reducer(reducer_name, args)?;

        // Pop frame
        let mut reducer_frame = self.call_stack.pop().unwrap();

        // Apply transaction
        for insert in reducer_frame.transaction.inserts {
            let table = module.tables.get_mut(&insert.table_name).ok_or_else(|| {
                IntersticeError::TableNotFound {
                    module: module_name.into(),
                    table: insert.table_name.clone(),
                }
            })?;
            if !validate_row(&insert.row, &table.schema) {
                return Err(IntersticeError::InvalidRow {
                    module: module_name.into(),
                    table: insert.table_name.clone(),
                });
            }
            table.rows.push(insert.row.clone());
            reducer_frame
                .emitted_events
                .push(TableEventInstance::TableInsertEvent {
                    module_name: module_name.into(),
                    table_name: insert.table_name,
                    inserted_row: insert.row,
                });
        }
        for update in reducer_frame.transaction.updates {
            let table = module.tables.get_mut(&update.table_name).ok_or_else(|| {
                IntersticeError::TableNotFound {
                    module: module_name.into(),
                    table: update.table_name.clone(),
                }
            })?;
            let mut old_row = None;
            for row in table.rows.iter_mut() {
                if row.primary_key == update.row.primary_key {
                    old_row = Some(row.clone());
                    *row = update.row.clone();
                    break;
                }
            }
            if let Some(old_row) = old_row {
                reducer_frame
                    .emitted_events
                    .push(TableEventInstance::TableUpdateEvent {
                        module_name: module_name.into(),
                        table_name: update.table_name,
                        old_row,
                        new_row: update.row,
                    });
            }
        }
        for delete in reducer_frame.transaction.deletes {
            let table = module.tables.get_mut(&delete.table_name).ok_or_else(|| {
                IntersticeError::TableNotFound {
                    module: module_name.into(),
                    table: delete.table_name.clone(),
                }
            })?;
            let deleted_row_idx = table
                .rows
                .iter()
                .position(|row| row.primary_key == delete.key);

            if let Some(deleted_row_idx) = deleted_row_idx {
                let deleted_row = table.rows.swap_remove(deleted_row_idx);
                reducer_frame
                    .emitted_events
                    .push(TableEventInstance::TableDeleteEvent {
                        module_name: module_name.into(),
                        table_name: delete.table_name,
                        deleted_row,
                    });
            }
        }

        Ok((result, reducer_frame.emitted_events))
    }
}
