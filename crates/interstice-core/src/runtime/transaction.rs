use std::convert::TryInto;
use std::collections::HashMap;
use std::sync::Arc;

use interstice_abi::{IndexKey, PersistenceKind, Row};

use crate::{
    error::IntersticeError,
    persistence::{LogOperation, SnapshotPlan},
    runtime::{
        Runtime,
        event::EventInstance,
        module::Module,
        table::{Table, TableOverlay},
    },
};

#[derive(Debug, Clone, serde::Serialize)]
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
    /// Restore marker (§6.7): the writes staged before it turn the table
    /// into its state at `to`. It applies nothing; on a logged table it
    /// becomes a `Restore{to}` log entry that tags the commit.
    Restore {
        module_name: String,
        table_name: String,
        to: u64,
    },
}

impl Runtime {
    /// Hot path: apply all transactions from a single reducer call, holding
    /// `module.tables` locked exactly once for the full batch. Avoids the
    /// per-transaction lock/unlock overhead, and skips the `Row::clone()`
    /// when the table is ephemeral and there are no subscribers.
    ///
    /// The commit is all-or-nothing: every write was validated against the
    /// reducer's merged view when it was issued (see `TableOverlay`). If any
    /// touched table was modified by someone else since, the whole batch is
    /// re-validated against the current committed state *before* anything is
    /// applied, under the same lock, and rejected as a unit on conflict.
    ///
    /// On success the table events and then `calls` (the released outbox) are
    /// pushed into the event channel *under the write lock*, so the channel
    /// order is commit order (DESIGN §6.6). Nothing is emitted on failure.
    pub(crate) fn apply_all_transactions(
        &self,
        transactions: Vec<Transaction>,
        overlays: &HashMap<String, TableOverlay>,
        module: &Arc<Module>,
        calls: Option<EventInstance>,
        tx: (u64, u64),
    ) -> Result<(), IntersticeError> {
        if transactions.is_empty() {
            // No commit to order against.
            if let Some(calls) = calls {
                self.emit_event(calls);
            }
            return Ok(());
        }

        let has_subscriptions = self
            .active_subscription_count
            .load(std::sync::atomic::Ordering::Relaxed)
            > 0;

        let mut events = Vec::new();
        // Deferred snapshot work: collected under the lock, executed after.
        let mut snapshots: Vec<(SnapshotPlan, Vec<Row>)> = Vec::new();

        {
            let mut tables = module.tables.write();

            let stale = overlays.iter().any(|(name, ov)| {
                tables
                    .get(name)
                    .is_none_or(|t| t.version() != ov.base_version)
            });
            if stale {
                validate_batch(&transactions, &tables)?;
            }

            for transaction in transactions {
                match transaction {
                    Transaction::Insert {
                        module_name,
                        table_name,
                        new_row,
                    } => {
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
                                        &module_name,
                                        &table_name,
                                        &pk,
                                        &row,
                                    )?;
                                }
                                PersistenceKind::Logged => {
                                    let pk = row_primary_key(&row)?;
                                    if let Some(plan) = self.log_logged(
                                        &module_name,
                                        table,
                                        LogOperation::Insert {
                                            primary_key: pk,
                                            row: Some(row.clone()),
                                        },
                                        tx,
                                    )? {
                                        snapshots.push((plan, table.snapshot_rows()));
                                    }
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
                                    &module_name,
                                    &table_name,
                                    &pk,
                                    &update_row,
                                )?;
                            }
                            PersistenceKind::Logged => {
                                let pk = row_primary_key(&update_row)?;
                                if let Some(plan) = self.log_logged(
                                    &module_name,
                                    table,
                                    LogOperation::Update {
                                        primary_key: pk,
                                        row: Some(update_row),
                                    },
                                    tx,
                                )? {
                                    snapshots.push((plan, table.snapshot_rows()));
                                }
                            }
                            PersistenceKind::Ephemeral => {}
                        }
                    }

                    Transaction::Delete {
                        module_name,
                        table_name,
                        deleted_row_id,
                    } => {
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
                                    &module_name,
                                    &table_name,
                                    &deleted_row_id,
                                )?;
                            }
                            PersistenceKind::Logged => {
                                if let Some(plan) = self.log_logged(
                                    &module_name,
                                    table,
                                    LogOperation::Delete {
                                        primary_key: deleted_row_id,
                                    },
                                    tx,
                                )? {
                                    snapshots.push((plan, table.snapshot_rows()));
                                }
                            }
                            PersistenceKind::Ephemeral => {}
                        }
                    }

                    Transaction::Clear {
                        module_name,
                        table_name,
                    } => {
                        let table = tables.get_mut(&table_name).ok_or_else(|| {
                            IntersticeError::TableNotFound {
                                module_name: module_name.clone(),
                                table_name: table_name.clone(),
                            }
                        })?;
                        let persistence_kind = table.schema.persistence.clone();
                        let deleted_rows =
                            if has_subscriptions { table.snapshot_rows() } else { vec![] };
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
                                self.persistence
                                    .persist_stateful_clear(&module_name, &table_name)?;
                            }
                            PersistenceKind::Logged => {
                                if let Some(plan) = self.log_logged(
                                    &module_name,
                                    table,
                                    LogOperation::Clear,
                                    tx,
                                )? {
                                    snapshots.push((plan, table.snapshot_rows()));
                                }
                            }
                            PersistenceKind::Ephemeral => {}
                        }
                    }

                    Transaction::Restore { module_name, table_name, to } => {
                        let table = tables.get_mut(&table_name).ok_or_else(|| {
                            IntersticeError::TableNotFound {
                                module_name: module_name.clone(),
                                table_name: table_name.clone(),
                            }
                        })?;
                        if table.schema.persistence == PersistenceKind::Logged {
                            if let Some(plan) = self.log_logged(
                                &module_name,
                                table,
                                LogOperation::Restore { to },
                                tx,
                            )? {
                                snapshots.push((plan, table.snapshot_rows()));
                            }
                        }
                    }
                }
            }

            // Event tables: evict beyond their retention bound.
            for table in tables.values_mut() {
                table.enforce_retention();
            }

            for event in events.into_iter().chain(calls) {
                self.emit_event(event);
            }
        } // tables lock released here

        // Execute any deferred logged snapshots outside the tables lock.
        for (plan, rows) in snapshots {
            self.persistence.snapshot_logged_table(plan, rows)?;
        }

        Ok(())
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
        let log_operation: LogOperation;
        let mut logged_snapshot_plan: Option<SnapshotPlan> = None;
        let mut logged_snapshot_rows: Option<Vec<Row>> = None;

        match transaction {
            // Only a module's own reducers restore, through
            // `apply_all_transactions`.
            Transaction::Restore { .. } => return Ok(events),
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
                        modules
                            .get(&module_name)
                            .ok_or_else(|| {
                                IntersticeError::ModuleNotFound(
                                    module_name.clone(),
                                    format!(
                                        "When trying to insert into table '{}' from '{}'",
                                        table_name.clone(),
                                        module_name.clone()
                                    ),
                                )
                            })?
                            .clone()
                    }
                };

                let mut tables = module.tables.write();
                let table =
                    tables
                        .get_mut(&table_name)
                        .ok_or_else(|| IntersticeError::TableNotFound {
                            module_name: module_name.clone(),
                            table_name: table_name.clone(),
                        })?;
                persistence_kind = table.schema.persistence.clone();

                if module_arc.is_some() {
                    // Reducer path: the insert was validated in handle_insert_row / validate_batch
                    table.insert_trusted(new_row.clone())?;
                } else {
                    table.insert(new_row.clone())?;
                }
                table.enforce_retention();

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
                            log_operation = LogOperation::Insert {
                                primary_key: pk,
                                row: Some(new_row.clone()),
                            };
                            if let Some(plan) = self.log_logged(
                                &module_name_for_persistence,
                                table,
                                log_operation,
                                self.tx_clock.next(crate::runtime::deterministic_random::wall_clock_ms()),
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
                        modules
                            .get(&module_name)
                            .ok_or_else(|| {
                                IntersticeError::ModuleNotFound(
                                    module_name.clone(),
                                    format!(
                                        "When trying to update table '{}' from '{}'",
                                        table_name.clone(),
                                        module_name.clone()
                                    ),
                                )
                            })?
                            .clone()
                    }
                };

                let mut tables = module.tables.write();
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
                            log_operation = LogOperation::Update {
                                primary_key: pk,
                                row: Some(update_row.clone()),
                            };
                            if let Some(plan) = self.log_logged(
                                &module_name_for_persistence,
                                table,
                                log_operation,
                                self.tx_clock.next(crate::runtime::deterministic_random::wall_clock_ms()),
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
                        modules
                            .get(&module_name)
                            .ok_or_else(|| {
                                IntersticeError::ModuleNotFound(
                                    module_name.clone(),
                                    format!(
                                        "When trying to delete a row of table '{}' from '{}'",
                                        table_name.clone(),
                                        module_name.clone()
                                    ),
                                )
                            })?
                            .clone()
                    }
                };

                let mut tables = module.tables.write();
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
                            log_operation = LogOperation::Delete {
                                primary_key: deleted_row_id.clone(),
                            };
                            if let Some(plan) = self.log_logged(
                                &module_name_for_persistence,
                                table,
                                log_operation,
                                self.tx_clock.next(crate::runtime::deterministic_random::wall_clock_ms()),
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
                        modules
                            .get(&module_name)
                            .ok_or_else(|| {
                                IntersticeError::ModuleNotFound(
                                    module_name.clone(),
                                    format!(
                                        "When trying to clear table '{}' from '{}'",
                                        table_name, module_name
                                    ),
                                )
                            })?
                            .clone()
                    }
                };

                let mut tables = module.tables.write();
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
                            log_operation = LogOperation::Clear;
                            if let Some(plan) = self.log_logged(
                                &module_name_for_persistence,
                                table,
                                log_operation,
                                self.tx_clock.next(crate::runtime::deterministic_random::wall_clock_ms()),
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
            if let (Some(plan), Some(rows)) =
                (logged_snapshot_plan.take(), logged_snapshot_rows.take())
            {
                self.persistence.snapshot_logged_table(plan, rows)?;
            }
        }

        Ok(events)
    }
}

impl Runtime {
    /// Append one entry to a logged table's log (§6.7), stamped with the
    /// transaction that wrote it.
    fn log_logged(
        &self,
        module_name: &str,
        table: &Table,
        operation: LogOperation,
        tx: (u64, u64),
    ) -> Result<Option<SnapshotPlan>, IntersticeError> {
        self.persistence.record_logged_operation(
            module_name,
            &table.schema.name,
            operation,
            tx,
            table.scan().len(),
            table.schema.history,
        )
    }
}

fn row_primary_key(row: &Row) -> Result<IndexKey, IntersticeError> {
    row.primary_key
        .clone()
        .try_into()
        .map_err(|err| IntersticeError::Internal(format!("Failed to convert primary key: {}", err)))
}

/// Replay a batch on a scratch overlay over the committed tables, checking
/// every operation without mutating anything.
fn validate_batch(
    transactions: &[Transaction],
    tables: &HashMap<String, Table>,
) -> Result<(), IntersticeError> {
    let mut scratch: HashMap<&str, TableOverlay> = HashMap::new();
    for transaction in transactions {
        let (module_name, table_name) = match transaction {
            Transaction::Insert { module_name, table_name, .. }
            | Transaction::Update { module_name, table_name, .. }
            | Transaction::Delete { module_name, table_name, .. }
            | Transaction::Clear { module_name, table_name } => (module_name, table_name),
            // Its writes were diffed against a state that has since changed.
            Transaction::Restore { table_name, .. } => {
                return Err(IntersticeError::Internal(format!(
                    "restore of '{table_name}' conflicted with a concurrent commit"
                )));
            }
        };
        let table = tables
            .get(table_name)
            .ok_or_else(|| IntersticeError::TableNotFound {
                module_name: module_name.clone(),
                table_name: table_name.clone(),
            })?;
        let ov = scratch
            .entry(table_name.as_str())
            .or_insert_with(|| TableOverlay::new(table.version()));
        match transaction {
            Transaction::Insert { new_row, .. } => {
                table.ov_validate_insert(Some(ov), new_row)?;
                table.ov_record(ov, row_primary_key(new_row)?, Some(new_row.clone()))?;
            }
            Transaction::Update { update_row, .. } => {
                table.ov_validate_update(Some(ov), update_row)?;
                table.ov_record(ov, row_primary_key(update_row)?, Some(update_row.clone()))?;
            }
            Transaction::Delete { deleted_row_id, .. } => {
                table.ov_validate_delete(Some(ov), deleted_row_id)?;
                table.ov_record(ov, deleted_row_id.clone(), None)?;
            }
            Transaction::Clear { .. } => table.ov_record_clear(ov),
            Transaction::Restore { .. } => unreachable!(),
        }
    }
    Ok(())
}
