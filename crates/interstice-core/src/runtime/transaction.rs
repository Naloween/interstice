use interstice_abi::{IndexKey, Row, decode, encode};
use serde::{Deserialize, Serialize};

use crate::{
    error::IntersticeError,
    runtime::{Runtime, event::EventInstance},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Transaction {
    Insert {
        module_name: String,
        table_name: String,
        new_row: Row,
    },
    Update {
        module_name: String,
        table_name: String,
        update_row: Row,
    },
    Delete {
        module_name: String,
        table_name: String,
        deleted_row_id: IndexKey,
    },
}

impl Transaction {
    pub fn encode(&self) -> Result<Vec<u8>, IntersticeError> {
        encode(&self).map_err(|err| {
            IntersticeError::Internal(format!("Couldn't encode transaction: {}", err))
        })
    }
    pub fn decode(bytes: &[u8]) -> Result<Transaction, IntersticeError> {
        decode(bytes).map_err(|err| {
            IntersticeError::Internal(format!("Couldn't decode transaction: {}", err))
        })
    }
}

impl Runtime {
    pub(crate) fn apply_transaction(
        &self,
        transaction: Transaction,
        log_transaction: bool,
    ) -> Result<Vec<EventInstance>, IntersticeError> {
        let transaction_to_log = transaction.clone();

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
                table.insert(new_row.clone())?;
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

                let old_row = table.update(update_row.clone())?;
                events.push(EventInstance::TableUpdateEvent {
                    module_name,
                    table_name,
                    old_row,
                    new_row: update_row,
                });
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

                match table.delete(&deleted_row_id) {
                    Ok(deleted_row) => {
                        events.push(EventInstance::TableDeleteEvent {
                            module_name,
                            table_name,
                            deleted_row,
                        });
                    }
                    Err(_err) => {} // If the row to delete is not found, we won't emit an event
                }
            }
        };

        if log_transaction {
            self.transaction_logs
                .lock()
                .unwrap()
                .append(&transaction_to_log)?;
        }

        return Ok(events);
    }
}
