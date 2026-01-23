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
            reducer_frame.emitted_events.push(TableEventInstance {
                module_name: module_name.into(),
                table_name: insert.table_name,
                event: TableEvent::Insert,
                row: insert.row,
            });
        }
        for update in reducer_frame.transaction.updates {
            let table = module.tables.get_mut(&update.table_name).ok_or_else(|| {
                IntersticeError::TableNotFound {
                    module: module_name.into(),
                    table: update.table_name.clone(),
                }
            })?;
            for row in table.rows.iter_mut() {
                if row.primary_key == update.row.primary_key {
                    *row = update.row.clone();
                    break;
                }
            }
            reducer_frame.emitted_events.push(TableEventInstance {
                module_name: module_name.into(),
                table_name: update.table_name,
                event: TableEvent::Update,
                row: update.row,
            });
        }
        for delete in reducer_frame.transaction.deletes {
            let table = module.tables.get_mut(&delete.table_name).ok_or_else(|| {
                IntersticeError::TableNotFound {
                    module: module_name.into(),
                    table: delete.table_name.clone(),
                }
            })?;
            if let Some(deleted_row) = table.rows.iter().find(|row| row.primary_key == delete.key) {
                reducer_frame.emitted_events.push(TableEventInstance {
                    module_name: module_name.into(),
                    table_name: delete.table_name,
                    event: TableEvent::Update,
                    row: deleted_row.clone(),
                });
            }
            table.rows.retain(|row| row.primary_key != delete.key);
        }

        Ok((result, reducer_frame.emitted_events))
    }
}
