use crate::{
    error::IntersticeError, runtime::Runtime, runtime::event::EventInstance,
    runtime::transaction::Transaction,
};
use interstice_abi::ReducerContext;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallFrameKind {
    Reducer,
    Query,
}

#[derive(Debug)]
pub struct CallFrame {
    pub module: String,
    pub kind: CallFrameKind,
    pub transactions: Vec<Transaction>,
}

impl CallFrame {
    pub fn new(module: String, kind: CallFrameKind) -> Self {
        Self {
            module,
            kind,
            transactions: Vec::new(),
        }
    }
}

impl Runtime {
    pub(crate) async fn call_reducer(
        &self,
        module_name: &str,
        reducer_name: &str,
        args: impl Serialize,
    ) -> Result<(), IntersticeError> {
        // Lookup module
        let module = {
            let mut modules = self.modules.lock().unwrap();
            modules
                .get_mut(module_name)
                .ok_or_else(|| {
                    IntersticeError::ModuleNotFound(
                        module_name.into(),
                        format!(
                            "When trying to invoke reducer '{}' from '{}'",
                            reducer_name, module_name
                        ),
                    )
                })?
                .clone()
        };

        // Check that reducer exists in schema
        module
            .schema
            .reducers
            .iter()
            .find(|r| r.name == reducer_name)
            .ok_or_else(|| IntersticeError::ReducerNotFound {
                module: module_name.into(),
                reducer: reducer_name.into(),
            })?;

        // Detect cycles (no module already called before)
        if self
            .call_stack
            .lock()
            .unwrap()
            .iter()
            .any(|f| f.module == module_name)
        {
            return Err(IntersticeError::ReducerCycle {
                module: module_name.into(),
                reducer: reducer_name.into(),
            });
        }

        // Push frame
        self.call_stack
            .lock()
            .unwrap()
            .push(CallFrame::new(module_name.into(), CallFrameKind::Reducer));

        // Call WASM function
        let reducer_context = ReducerContext::new();
        module
            .call_reducer(reducer_name, (reducer_context, args))
            .await?;

        // Pop frame
        let reducer_frame = self.call_stack.lock().unwrap().pop().unwrap();

        // Apply transactions
        let mut emitted_events = Vec::new();
        for transaction in reducer_frame.transactions {
            emitted_events.append(&mut self.apply_transaction(transaction)?);
        }

        // Send events
        for ev in emitted_events {
            self.event_sender.send(ev).unwrap();
        }

        Ok(())
    }

    pub(crate) fn apply_transaction(
        &self,
        transaction: Transaction,
    ) -> Result<Vec<EventInstance>, IntersticeError> {
        // Add transaction to the logs
        self.transaction_logs.lock().unwrap().append(&transaction)?;

        // Apply transactions locally and collect events
        let mut events = Vec::new();
        match transaction {
            Transaction::Insert {
                module_name,
                table_name,
                new_row,
            } => {
                let mut modules = self.modules.lock().unwrap();
                let module = modules.get_mut(&module_name).ok_or_else(|| {
                    IntersticeError::ModuleNotFound(
                        module_name.clone(),
                        format!(
                            "When trying to insert into table '{}' from '{}'",
                            table_name.clone(),
                            module_name.clone()
                        ),
                    )
                })?;
                let mut tables = module.tables.lock().unwrap();
                let table =
                    tables
                        .get_mut(&table_name)
                        .ok_or_else(|| IntersticeError::TableNotFound {
                            module_name: module_name.clone(),
                            table_name: table_name.clone(),
                        })?;
                table.rows.push(new_row.clone());
                events.push(EventInstance::TableInsertEvent {
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
                let mut modules = self.modules.lock().unwrap();
                let module = modules.get_mut(&module_name).ok_or_else(|| {
                    IntersticeError::ModuleNotFound(
                        module_name.clone(),
                        format!(
                            "When trying to update table '{}' from '{}'",
                            table_name.clone(),
                            module_name.clone()
                        ),
                    )
                })?;
                let mut tables = module.tables.lock().unwrap();
                let table =
                    tables
                        .get_mut(&table_name)
                        .ok_or_else(|| IntersticeError::TableNotFound {
                            module_name: module_name.clone(),
                            table_name: table_name.clone(),
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
                    events.push(EventInstance::TableUpdateEvent {
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
                let mut modules = self.modules.lock().unwrap();
                let module = modules.get_mut(&module_name).ok_or_else(|| {
                    IntersticeError::ModuleNotFound(
                        module_name.clone(),
                        format!(
                            "When trying to delete a row of table '{}' from '{}'",
                            table_name.clone(),
                            module_name.clone()
                        ),
                    )
                })?;
                let mut tables = module.tables.lock().unwrap();
                let table =
                    tables
                        .get_mut(&table_name)
                        .ok_or_else(|| IntersticeError::TableNotFound {
                            module_name: module_name.clone(),
                            table_name: table_name.clone(),
                        })?;
                let deleted_row_idx = table
                    .rows
                    .iter()
                    .position(|row| row.primary_key == deleted_row_id);

                if let Some(deleted_row_idx) = deleted_row_idx {
                    let deleted_row = table.rows.swap_remove(deleted_row_idx);
                    events.push(EventInstance::TableDeleteEvent {
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
