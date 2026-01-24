use crate::runtime::Runtime;
use crate::runtime::event::TableEventInstance;
use crate::runtime::transaction::Transaction;
use crate::{error::IntersticeError, runtime::table::validate_row};
use interstice_abi::{DeleteRowRequest, InsertRowRequest, IntersticeValue, UpdateRowRequest};

#[derive(Debug)]
pub struct ReducerFrame {
    pub module: String,
    pub reducer: String,
    pub transactions: Vec<Transaction>,
    pub emitted_events: Vec<TableEventInstance>,
}

impl ReducerFrame {
    pub fn new(module: String, reducer: String) -> Self {
        Self {
            module,
            reducer,
            transactions: Vec::new(),
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
        for transaction in reducer_frame.transactions {
            reducer_frame
                .emitted_events
                .append(&mut self.apply_transaction(transaction)?);
        }

        Ok((result, reducer_frame.emitted_events))
    }

    pub(crate) fn apply_transaction(
        &mut self,
        transaction: Transaction,
    ) -> Result<Vec<TableEventInstance>, IntersticeError> {
        // Add transaction to the logs
        self.transaction_logs.append(&transaction)?;

        // Apply transactions locally and collect events
        let mut events = Vec::new();
        match transaction {
            Transaction::Insert {
                module_name,
                table_name,
                new_row,
            } => {
                let module = self
                    .modules
                    .get_mut(&module_name)
                    .ok_or_else(|| IntersticeError::ModuleNotFound(module_name.clone()))?;
                let table = module.tables.get_mut(&table_name).ok_or_else(|| {
                    IntersticeError::TableNotFound {
                        module_name: module_name.clone(),
                        table_name: table_name.clone(),
                    }
                })?;
                if !validate_row(&new_row, &table.schema) {
                    return Err(IntersticeError::InvalidRow {
                        module: module_name.clone(),
                        table: table_name.clone(),
                    });
                }
                table.rows.push(new_row.clone());
                events.push(TableEventInstance::TableInsertEvent {
                    module_name,
                    table_name,
                    inserted_row: new_row,
                });
            }

            Transaction::Update {
                module_name,
                table_name,
                update_row,
            } => {
                let module = self
                    .modules
                    .get_mut(&module_name)
                    .ok_or_else(|| IntersticeError::ModuleNotFound(module_name.clone()))?;
                let table = module.tables.get_mut(&table_name).ok_or_else(|| {
                    IntersticeError::TableNotFound {
                        module_name: module_name.clone(),
                        table_name: table_name.clone(),
                    }
                })?;
                let mut old_row = None;
                for row in table.rows.iter_mut() {
                    if row.primary_key == update_row.primary_key {
                        old_row = Some(row.clone());
                        *row = update_row.clone();
                        break;
                    }
                }
                if let Some(old_row) = old_row {
                    events.push(TableEventInstance::TableUpdateEvent {
                        module_name,
                        table_name,
                        old_row,
                        new_row: update_row,
                    });
                }
            }
            Transaction::Delete {
                module_name,
                table_name,
                deleted_row_id,
            } => {
                let module = self
                    .modules
                    .get_mut(&module_name)
                    .ok_or_else(|| IntersticeError::ModuleNotFound(module_name.clone()))?;
                let table = module.tables.get_mut(&table_name).ok_or_else(|| {
                    IntersticeError::TableNotFound {
                        module_name: module_name.clone(),
                        table_name: table_name.clone(),
                    }
                })?;
                let deleted_row_idx = table
                    .rows
                    .iter()
                    .position(|row| row.primary_key == deleted_row_id);

                if let Some(deleted_row_idx) = deleted_row_idx {
                    let deleted_row = table.rows.swap_remove(deleted_row_idx);
                    events.push(TableEventInstance::TableDeleteEvent {
                        module_name,
                        table_name,
                        deleted_row,
                    });
                }
            }
        };
        return Ok(events);
    }
}
