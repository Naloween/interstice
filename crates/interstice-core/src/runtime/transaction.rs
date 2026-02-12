use std::convert::TryInto;

use interstice_abi::{IndexKey, PersistenceKind, Row};

use crate::{
    error::IntersticeError,
    persistence::{LogOperation, SnapshotPlan},
    runtime::{Runtime, event::EventInstance},
};

#[derive(Debug)]
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

impl Runtime {
    pub(crate) fn apply_transaction(
        &self,
        transaction: Transaction,
        log_transaction: bool,
    ) -> Result<Vec<EventInstance>, IntersticeError> {
        let mut events = Vec::new();
        let module_name_for_persistence;
        let table_name_for_persistence;
        let persistence_kind;
        let mut log_operation: Option<LogOperation> = None;
        let mut logged_snapshot_plan: Option<SnapshotPlan> = None;
        let mut logged_snapshot_rows: Option<Vec<Row>> = None;
        let mut stateful_rows: Option<Vec<Row>> = None;

        match transaction {
            Transaction::Insert {
                module_name,
                table_name,
                new_row,
            } => {
                module_name_for_persistence = module_name.clone();
                table_name_for_persistence = table_name.clone();

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
                persistence_kind = table.schema.persistence.clone();

                table.insert(new_row.clone())?;
                events.push(EventInstance::TableInsertEvent {
                    module_name,
                    table_name,
                    inserted_row: new_row.clone(),
                });

                if log_transaction {
                    if persistence_kind != PersistenceKind::Ephemeral {
                        let pk = row_primary_key(&new_row)?;
                        let row_for_log = if persistence_kind == PersistenceKind::Logged {
                            Some(new_row.clone())
                        } else {
                            None
                        };
                        log_operation = Some(LogOperation::Insert {
                            primary_key: pk,
                            row: row_for_log,
                        });
                    }

                    match persistence_kind {
                        PersistenceKind::Logged => {
                            if let Some(plan) = self.persistence.record_logged_operation(
                                &module_name_for_persistence,
                                &table_name_for_persistence,
                                log_operation
                                    .as_ref()
                                    .cloned()
                                    .expect("logged operation missing"),
                            )? {
                                logged_snapshot_rows = Some(table.snapshot_rows());
                                logged_snapshot_plan = Some(plan);
                            }
                        }
                        PersistenceKind::Stateful => {
                            stateful_rows = Some(table.snapshot_rows());
                        }
                        PersistenceKind::Ephemeral => {}
                    }
                }
            }

            Transaction::Update {
                module_name,
                table_name,
                update_row,
            } => {
                module_name_for_persistence = module_name.clone();
                table_name_for_persistence = table_name.clone();

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
                persistence_kind = table.schema.persistence.clone();

                let old_row = table.update(update_row.clone())?;
                events.push(EventInstance::TableUpdateEvent {
                    module_name,
                    table_name,
                    old_row,
                    new_row: update_row.clone(),
                });

                if log_transaction {
                    if persistence_kind != PersistenceKind::Ephemeral {
                        let pk = row_primary_key(&update_row)?;
                        let row_for_log = if persistence_kind == PersistenceKind::Logged {
                            Some(update_row.clone())
                        } else {
                            None
                        };
                        log_operation = Some(LogOperation::Update {
                            primary_key: pk,
                            row: row_for_log,
                        });
                    }

                    match persistence_kind {
                        PersistenceKind::Logged => {
                            if let Some(plan) = self.persistence.record_logged_operation(
                                &module_name_for_persistence,
                                &table_name_for_persistence,
                                log_operation
                                    .as_ref()
                                    .cloned()
                                    .expect("logged operation missing"),
                            )? {
                                logged_snapshot_rows = Some(table.snapshot_rows());
                                logged_snapshot_plan = Some(plan);
                            }
                        }
                        PersistenceKind::Stateful => {
                            stateful_rows = Some(table.snapshot_rows());
                        }
                        PersistenceKind::Ephemeral => {}
                    }
                }
            }

            Transaction::Delete {
                module_name,
                table_name,
                deleted_row_id,
            } => {
                module_name_for_persistence = module_name.clone();
                table_name_for_persistence = table_name.clone();

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
                persistence_kind = table.schema.persistence.clone();

                if let Ok(deleted_row) = table.delete(&deleted_row_id) {
                    events.push(EventInstance::TableDeleteEvent {
                        module_name,
                        table_name,
                        deleted_row,
                    });
                }

                if log_transaction {
                    if persistence_kind != PersistenceKind::Ephemeral {
                        log_operation = Some(LogOperation::Delete {
                            primary_key: deleted_row_id.clone(),
                        });
                    }

                    match persistence_kind {
                        PersistenceKind::Logged => {
                            if let Some(plan) = self.persistence.record_logged_operation(
                                &module_name_for_persistence,
                                &table_name_for_persistence,
                                log_operation
                                    .as_ref()
                                    .cloned()
                                    .expect("logged operation missing"),
                            )? {
                                logged_snapshot_rows = Some(table.snapshot_rows());
                                logged_snapshot_plan = Some(plan);
                            }
                        }
                        PersistenceKind::Stateful => {
                            stateful_rows = Some(table.snapshot_rows());
                        }
                        PersistenceKind::Ephemeral => {}
                    }
                }
            }
        }

        if log_transaction {
            match persistence_kind {
                PersistenceKind::Logged => {
                    if let (Some(plan), Some(rows)) =
                        (logged_snapshot_plan.take(), logged_snapshot_rows.take())
                    {
                        self.persistence.snapshot_logged_table(plan, rows)?;
                    }
                }
                PersistenceKind::Stateful => {
                    if let (Some(operation), Some(rows)) =
                        (log_operation.take(), stateful_rows.take())
                    {
                        self.persistence.persist_stateful_operation(
                            &module_name_for_persistence,
                            &table_name_for_persistence,
                            operation,
                            rows,
                        )?;
                    }
                }
                PersistenceKind::Ephemeral => {}
            }
        }

        Ok(events)
    }
}

fn row_primary_key(row: &Row) -> Result<IndexKey, IntersticeError> {
    row.primary_key
        .clone()
        .try_into()
        .map_err(|err| IntersticeError::Internal(format!("Failed to convert primary key: {}", err)))
}
