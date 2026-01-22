use crate::error::IntersticeError;
use crate::runtime::Runtime;
use interstice_abi::{DeleteRowRequest, InsertRowRequest, PrimitiveValue, UpdateRowRequest};

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
        }
    }
}

impl Runtime {
    pub fn invoke_reducer(
        &mut self,
        module_name: &str,
        reducer_name: &str,
        args: PrimitiveValue,
    ) -> Result<PrimitiveValue, IntersticeError> {
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
        let reducer_frame = self.call_stack.pop().unwrap();

        // Apply transaction
        for insert in reducer_frame.transaction.inserts {
            module
                .tables
                .get_mut(&insert.table)
                .ok_or_else(|| IntersticeError::TableNotFound {
                    module: module_name.into(),
                    table: insert.table.clone(),
                })?
                .rows
                .push(insert.row);
        }
        for update in reducer_frame.transaction.updates {
            let table = module.tables.get_mut(&update.table).ok_or_else(|| {
                IntersticeError::TableNotFound {
                    module: module_name.into(),
                    table: update.table.clone(),
                }
            })?;
            for row in table.rows.iter_mut() {
                if row.primary_key == update.row.primary_key {
                    *row = update.row;
                    break;
                }
            }
        }
        for delete in reducer_frame.transaction.deletes {
            let table = module.tables.get_mut(&delete.table).ok_or_else(|| {
                IntersticeError::TableNotFound {
                    module: module_name.into(),
                    table: delete.table.clone(),
                }
            })?;
            table.rows.retain(|row| row.primary_key != delete.key);
        }

        Ok(result)
    }
}
