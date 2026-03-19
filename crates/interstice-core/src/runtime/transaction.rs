use std::convert::TryInto;
use std::sync::Arc;

use interstice_abi::{IndexKey, PersistenceKind, Row};

use crate::{
    error::IntersticeError,
    persistence::{LogOperation, SnapshotPlan},
    runtime::{Runtime, event::EventInstance, module::Module},
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
    Clear {
        module_name: String,
        table_name: String,
    },
}

impl Runtime {
    /// Hot path: apply all transactions from a single reducer call, holding
    /// `module.tables` locked exactly once for the full batch. Avoids the
    /// per-transaction lock/unlock overhead, and skips the `Row::clone()`
    /// when the table is ephemeral and there are no subscribers.
    pub(crate) fn apply_all_transactions(
        &self,
        transactions: Vec<Transaction>,
        module: &Arc<Module>,
    ) -> Result<Vec<EventInstance>, IntersticeError> {
        if transactions.is_empty() {
            return Ok(Vec::new());
        }

        let has_subscriptions = self
            .active_subscription_count
            .load(std::sync::atomic::Ordering::Relaxed)
            > 0;

        let mut events = Vec::new();
        // Deferred snapshot work: collected under the lock, executed after.
        let mut snapshots: Vec<(SnapshotPlan, Vec<Row>)> = Vec::new();

        {
            let mut tables = module.tables.lock();

            for transaction in transactions {
                match transaction {
                    Transaction::Insert { module_name, table_name, new_row } => {
                        let table = tables.get_mut(&table_name).ok_or_else(|| {
                            IntersticeError::TableNotFound {
                                module_name: module_name.clone(),
                                table_name: table_name.clone(),
                            }
                        })?;
                        let persistence_kind = table.schema.persistence.clone();

                        // Clone only when actually needed after the insert.
                        // Hot path (ephemeral + no subscriptions): zero allocations.
                        let need_copy = has_subscriptions
                            || !matches!(persistence_kind, PersistenceKind::Ephemeral);
                        let row_copy = need_copy.then(|| new_row.clone());
                        table.insert_trusted(new_row)?;

                        if let Some(row) = row_copy {
                            if has_subscriptions {
                                events.push(EventInstance::TableInsertEvent {
                                    source_node_id: None,
                                    module_name: module_name.clone(),
                                    table_name: table_name.clone(),
                                    inserted_row: row.clone(),
                                });
                            }
                            match &persistence_kind {
                                PersistenceKind::Stateful => {
                                    let pk = row_primary_key(&row)?;
                                    self.persistence.persist_stateful_insert(
                                        &module_name, &table_name, &pk, &row,
                                    )?;
                                }
                                PersistenceKind::Logged => {
                                    let pk = row_primary_key(&row)?;
                                    if let Some(plan) = self.persistence.record_logged_operation(
                                        &module_name,
                                        &table_name,
                                        LogOperation::Insert { primary_key: pk, row: Some(row.clone()) },
                                    )? {
                                        snapshots.push((plan, table.snapshot_rows()));
                                    }
                                }
                                PersistenceKind::Ephemeral => {}
                            }
                        }
                    }

                    Transaction::Update { module_name, table_name, update_row } => {
                        let table = tables.get_mut(&table_name).ok_or_else(|| {
                            IntersticeError::TableNotFound {
                                module_name: module_name.clone(),
                                table_name: table_name.clone(),
                            }
                        })?;
                        let persistence_kind = table.schema.persistence.clone();
                        let old_row = table.update(update_row.clone())?;

                        if has_subscriptions {
                            events.push(EventInstance::TableUpdateEvent {
                                source_node_id: None,
                                module_name: module_name.clone(),
                                table_name: table_name.clone(),
                                old_row,
                                new_row: update_row.clone(),
                            });
                        }
                        match persistence_kind {
                            PersistenceKind::Stateful => {
                                let pk = row_primary_key(&update_row)?;
                                self.persistence.persist_stateful_update(
                                    &module_name, &table_name, &pk, &update_row,
                                )?;
                            }
                            PersistenceKind::Logged => {
                                let pk = row_primary_key(&update_row)?;
                                if let Some(plan) = self.persistence.record_logged_operation(
                                    &module_name,
                                    &table_name,
                                    LogOperation::Update { primary_key: pk, row: Some(update_row) },
                                )? {
                                    snapshots.push((plan, table.snapshot_rows()));
                                }
                            }
                            PersistenceKind::Ephemeral => {}
                        }
                    }

                    Transaction::Delete { module_name, table_name, deleted_row_id } => {
                        let table = tables.get_mut(&table_name).ok_or_else(|| {
                            IntersticeError::TableNotFound {
                                module_name: module_name.clone(),
                                table_name: table_name.clone(),
                            }
                        })?;
                        let persistence_kind = table.schema.persistence.clone();

                        if let Ok(deleted_row) = table.delete(&deleted_row_id) {
                            if has_subscriptions {
                                events.push(EventInstance::TableDeleteEvent {
                                    source_node_id: None,
                                    module_name: module_name.clone(),
                                    table_name: table_name.clone(),
                                    deleted_row,
                                });
                            }
                        }
                        match persistence_kind {
                            PersistenceKind::Stateful => {
                                self.persistence.persist_stateful_delete(
                                    &module_name, &table_name, &deleted_row_id,
                                )?;
                            }
                            PersistenceKind::Logged => {
                                if let Some(plan) = self.persistence.record_logged_operation(
                                    &module_name,
                                    &table_name,
                                    LogOperation::Delete { primary_key: deleted_row_id },
                                )? {
                                    snapshots.push((plan, table.snapshot_rows()));
                                }
                            }
                            PersistenceKind::Ephemeral => {}
                        }
                    }

                    Transaction::Clear { module_name, table_name } => {
                        let table = tables.get_mut(&table_name).ok_or_else(|| {
                            IntersticeError::TableNotFound {
                                module_name: module_name.clone(),
                                table_name: table_name.clone(),
                            }
                        })?;
                        let persistence_kind = table.schema.persistence.clone();
                        let deleted_rows = if has_subscriptions { table.snapshot_rows() } else { vec![] };
                        table.clear();

                        for deleted_row in deleted_rows {
                            events.push(EventInstance::TableDeleteEvent {
                                source_node_id: None,
                                module_name: module_name.clone(),
                                table_name: table_name.clone(),
                                deleted_row,
                            });
                        }
                        match persistence_kind {
                            PersistenceKind::Stateful => {
                                self.persistence.persist_stateful_clear(&module_name, &table_name)?;
                            }
                            PersistenceKind::Logged => {
                                if let Some(plan) = self.persistence.record_logged_operation(
                                    &module_name,
                                    &table_name,
                                    LogOperation::Clear,
                                )? {
                                    snapshots.push((plan, table.snapshot_rows()));
                                }
                            }
                            PersistenceKind::Ephemeral => {}
                        }
                    }
                }
            }
        } // tables lock released here

        // Execute any deferred logged snapshots outside the tables lock.
        for (plan, rows) in snapshots {
            self.persistence.snapshot_logged_table(plan, rows)?;
        }

        Ok(events)
    }

    pub(crate) fn apply_transaction(
        &self,
        transaction: Transaction,
        log_transaction: bool,
        module_arc: Option<&Arc<Module>>,
    ) -> Result<Vec<EventInstance>, IntersticeError> {
        let has_subscriptions = self
            .active_subscription_count
            .load(std::sync::atomic::Ordering::Relaxed)
            > 0;

        let mut events = Vec::new();
        let module_name_for_persistence;
        let table_name_for_persistence;
        let persistence_kind;
        let mut log_operation: Option<LogOperation> = None;
        let mut logged_snapshot_plan: Option<SnapshotPlan> = None;
        let mut logged_snapshot_rows: Option<Vec<Row>> = None;

        match transaction {
            Transaction::Insert {
                module_name,
                table_name,
                new_row,
            } => {
                module_name_for_persistence = module_name.clone();
                table_name_for_persistence = table_name.clone();

                let module: Arc<Module> = match module_arc {
                    Some(m) => Arc::clone(m),
                    None => {
                        let modules = self.modules.lock();
                        modules.get(&module_name).ok_or_else(|| {
                            IntersticeError::ModuleNotFound(
                                module_name.clone(),
                                format!(
                                    "When trying to insert into table '{}' from '{}'",
                                    table_name.clone(),
                                    module_name.clone()
                                ),
                            )
                        })?.clone()
                    }
                };

                let mut tables = module.tables.lock();
                let table =
                    tables
                        .get_mut(&table_name)
                        .ok_or_else(|| IntersticeError::TableNotFound {
                            module_name: module_name.clone(),
                            table_name: table_name.clone(),
                        })?;
                persistence_kind = table.schema.persistence.clone();

                if module_arc.is_some() {
                    // Reducer path: validate_insert was already called in handle_insert_row
                    table.insert_trusted(new_row.clone())?;
                } else {
                    table.insert(new_row.clone())?;
                }

                if has_subscriptions {
                    events.push(EventInstance::TableInsertEvent {
                        source_node_id: None,
                        module_name,
                        table_name,
                        inserted_row: new_row.clone(),
                    });
                }

                if log_transaction {
                    match persistence_kind {
                        PersistenceKind::Logged => {
                            let pk = row_primary_key(&new_row)?;
                            log_operation = Some(LogOperation::Insert {
                                primary_key: pk,
                                row: Some(new_row.clone()),
                            });
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
                            let pk = row_primary_key(&new_row)?;
                            self.persistence.persist_stateful_insert(
                                &module_name_for_persistence,
                                &table_name_for_persistence,
                                &pk,
                                &new_row,
                            )?;
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

                let module: Arc<Module> = match module_arc {
                    Some(m) => Arc::clone(m),
                    None => {
                        let modules = self.modules.lock();
                        modules.get(&module_name).ok_or_else(|| {
                            IntersticeError::ModuleNotFound(
                                module_name.clone(),
                                format!(
                                    "When trying to update table '{}' from '{}'",
                                    table_name.clone(),
                                    module_name.clone()
                                ),
                            )
                        })?.clone()
                    }
                };

                let mut tables = module.tables.lock();
                let table =
                    tables
                        .get_mut(&table_name)
                        .ok_or_else(|| IntersticeError::TableNotFound {
                            module_name: module_name.clone(),
                            table_name: table_name.clone(),
                        })?;
                persistence_kind = table.schema.persistence.clone();

                let old_row = table.update(update_row.clone())?;

                if has_subscriptions {
                    events.push(EventInstance::TableUpdateEvent {
                        source_node_id: None,
                        module_name,
                        table_name,
                        old_row,
                        new_row: update_row.clone(),
                    });
                }

                if log_transaction {
                    match persistence_kind {
                        PersistenceKind::Logged => {
                            let pk = row_primary_key(&update_row)?;
                            log_operation = Some(LogOperation::Update {
                                primary_key: pk,
                                row: Some(update_row.clone()),
                            });
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
                            let pk = row_primary_key(&update_row)?;
                            self.persistence.persist_stateful_update(
                                &module_name_for_persistence,
                                &table_name_for_persistence,
                                &pk,
                                &update_row,
                            )?;
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

                let module: Arc<Module> = match module_arc {
                    Some(m) => Arc::clone(m),
                    None => {
                        let modules = self.modules.lock();
                        modules.get(&module_name).ok_or_else(|| {
                            IntersticeError::ModuleNotFound(
                                module_name.clone(),
                                format!(
                                    "When trying to delete a row of table '{}' from '{}'",
                                    table_name.clone(),
                                    module_name.clone()
                                ),
                            )
                        })?.clone()
                    }
                };

                let mut tables = module.tables.lock();
                let table =
                    tables
                        .get_mut(&table_name)
                        .ok_or_else(|| IntersticeError::TableNotFound {
                            module_name: module_name.clone(),
                            table_name: table_name.clone(),
                        })?;
                persistence_kind = table.schema.persistence.clone();

                if let Ok(deleted_row) = table.delete(&deleted_row_id) {
                    if has_subscriptions {
                        events.push(EventInstance::TableDeleteEvent {
                            source_node_id: None,
                            module_name,
                            table_name,
                            deleted_row,
                        });
                    }
                }

                if log_transaction {
                    match persistence_kind {
                        PersistenceKind::Logged => {
                            log_operation = Some(LogOperation::Delete {
                                primary_key: deleted_row_id.clone(),
                            });
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
                            self.persistence.persist_stateful_delete(
                                &module_name_for_persistence,
                                &table_name_for_persistence,
                                &deleted_row_id,
                            )?;
                        }
                        PersistenceKind::Ephemeral => {}
                    }
                }
            }

            Transaction::Clear {
                module_name,
                table_name,
            } => {
                module_name_for_persistence = module_name.clone();
                table_name_for_persistence = table_name.clone();

                let module: Arc<Module> = match module_arc {
                    Some(m) => Arc::clone(m),
                    None => {
                        let modules = self.modules.lock();
                        modules.get(&module_name).ok_or_else(|| {
                            IntersticeError::ModuleNotFound(
                                module_name.clone(),
                                format!(
                                    "When trying to clear table '{}' from '{}'",
                                    table_name, module_name
                                ),
                            )
                        })?.clone()
                    }
                };

                let mut tables = module.tables.lock();
                let table =
                    tables
                        .get_mut(&table_name)
                        .ok_or_else(|| IntersticeError::TableNotFound {
                            module_name: module_name.clone(),
                            table_name: table_name.clone(),
                        })?;
                persistence_kind = table.schema.persistence.clone();

                let deleted_rows = table.snapshot_rows();
                table.clear();

                if has_subscriptions {
                    for deleted_row in deleted_rows {
                        events.push(EventInstance::TableDeleteEvent {
                            source_node_id: None,
                            module_name: module_name.clone(),
                            table_name: table_name.clone(),
                            deleted_row,
                        });
                    }
                }

                if log_transaction {
                    match persistence_kind {
                        PersistenceKind::Logged => {
                            log_operation = Some(LogOperation::Clear);
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
                            self.persistence.persist_stateful_clear(
                                &module_name_for_persistence,
                                &table_name_for_persistence,
                            )?;
                        }
                        PersistenceKind::Ephemeral => {}
                    }
                }
            }
        }

        if log_transaction {
            if let (Some(plan), Some(rows)) = (logged_snapshot_plan.take(), logged_snapshot_rows.take()) {
                self.persistence.snapshot_logged_table(plan, rows)?;
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
