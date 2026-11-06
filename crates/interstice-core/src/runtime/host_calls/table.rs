use crate::{
    runtime::transaction::Transaction,
    runtime::{Runtime, module::Module, reducer::{CallFrameKind, CALL_STACK}, table::TableOverlay},
};
use interstice_abi::{
    ClearTableRequest, ClearTableResponse, IndexKey, DeleteRowRequest, DeleteRowResponse, InsertRowRequest,
    InsertRowResponse, ModuleSchema, ModuleSelection, NodeSelection, ReducerTableRef,
    TableGetByPrimaryKeyRequest, TableGetByPrimaryKeyResponse, TableIndexScanRequest,
    TableIndexScanResponse, TableScanRequest, TableScanResponse, UpdateRowRequest, UpdateRowResponse,
};
use std::sync::Arc;

#[derive(Clone, Copy)]
enum TableAccessOp {
    Read,
    Insert,
    Update,
    Delete,
}

fn effective_reducer_table_ref(
    frame: &crate::runtime::reducer::CallFrame,
    module_selection: &ModuleSelection,
    physical_table: &str,
    replica_bindings: &[crate::runtime::ReplicaBinding],
) -> ReducerTableRef {
    let table_lower = physical_table.to_lowercase();
    for b in replica_bindings {
        if b.owner_module_name == frame.module && b.local_table_name == physical_table {
            return ReducerTableRef {
                node_selection: NodeSelection::Other(b.source_node_name.to_lowercase()),
                module_selection: ModuleSelection::Other(b.source_module_name.to_lowercase()),
                table_name: b.source_table_name.to_lowercase(),
            };
        }
    }
    ReducerTableRef {
        node_selection: NodeSelection::Current,
        module_selection: module_selection.clone(),
        table_name: table_lower,
    }
}

fn format_module_selection(m: &ModuleSelection) -> &str {
    match m {
        ModuleSelection::Current => "current",
        ModuleSelection::Other(s) => s.as_str(),
    }
}

fn format_reducer_table_ref(r: &ReducerTableRef) -> String {
    match &r.node_selection {
        NodeSelection::Current => format!(
            "current.{}.{}",
            format_module_selection(&r.module_selection),
            r.table_name
        ),
        NodeSelection::Other(n) => format!(
            "{}.{}.{}",
            n,
            format_module_selection(&r.module_selection),
            r.table_name
        ),
    }
}

impl Runtime {
    fn ensure_current_frame_table_access(
        &self,
        module_selection: &ModuleSelection,
        table_name: &str,
        op: TableAccessOp,
    ) -> Result<(), String> {
        CALL_STACK.with(|s| {
            let stack = s.borrow();
            let frame = stack
                .last()
                .ok_or_else(|| "No active call frame".to_string())?;
            if frame.kind == CallFrameKind::Query && !matches!(op, TableAccessOp::Read) {
                return Err(format!(
                    "{} not allowed in query context",
                    match op {
                        TableAccessOp::Read => "Read",
                        TableAccessOp::Insert => "Insert",
                        TableAccessOp::Update => "Update",
                        TableAccessOp::Delete => "Delete/Clear",
                    }
                ));
            }
            let bindings = self.replica_bindings.lock();
            let key = effective_reducer_table_ref(frame, module_selection, table_name, &bindings);
            let allowed = match op {
                TableAccessOp::Read => frame.table_access.reads.contains(&key),
                TableAccessOp::Insert => frame.table_access.inserts.contains(&key),
                TableAccessOp::Update => frame.table_access.updates.contains(&key),
                TableAccessOp::Delete => frame.table_access.deletes.contains(&key),
            };
            if allowed {
                Ok(())
            } else {
                let kind = match frame.kind {
                    CallFrameKind::Reducer => "Reducer",
                    CallFrameKind::Query => "Query",
                };
                Err(format!(
                    "{kind} '{}.{}' lacks {} permission for {} (declared access uses structured node.module.table refs)",
                    frame.module,
                    frame.reducer,
                    op_label(&op),
                    format_reducer_table_ref(&key),
                ))
            }
        })
    }

    fn selected_module_name(&self, module_selection: &ModuleSelection) -> Result<String, String> {
        match module_selection {
            ModuleSelection::Current => {
                CALL_STACK
                    .with(|s| s.borrow().last().map(|f| f.module.clone()))
                    .ok_or_else(|| "No active call frame".to_string())
            }
            ModuleSelection::Other(name) => Ok(name.clone()),
        }
    }

    /// Get module_arc for the current call frame (avoids modules HashMap lookup).
    fn current_frame_module_arc(&self) -> Result<Arc<Module>, String> {
        CALL_STACK
            .with(|s| s.borrow().last().map(|f| f.module_arc.clone()))
            .ok_or_else(|| "No active call frame".to_string())
    }

    /// The module a read selects. The frame's own module, named or not,
    /// resolves to the frame's `Module`.
    fn selected_module_arc(&self, selection: &ModuleSelection) -> Result<Arc<Module>, String> {
        let own = CALL_STACK.with(|s| {
            s.borrow().last().and_then(|f| match selection {
                ModuleSelection::Current => Some(f.module_arc.clone()),
                ModuleSelection::Other(name) if *name == f.module => Some(f.module_arc.clone()),
                ModuleSelection::Other(_) => None,
            })
        });
        match (own, selection) {
            (Some(module), _) => Ok(module),
            (None, ModuleSelection::Current) => Err("No active call frame".to_string()),
            (None, ModuleSelection::Other(name)) => self
                .modules
                .lock()
                .get(name)
                .cloned()
                .ok_or_else(|| "Module not found".to_string()),
        }
    }

    /// Validate a pending write against the frame's merged view (committed
    /// rows + earlier writes of this call), record it in the overlay and
    /// queue the transaction. Runs with `table` locked.
    fn stage_write(
        table: &crate::runtime::table::Table,
        table_name: &str,
        validate: impl FnOnce(&crate::runtime::table::Table, Option<&TableOverlay>) -> Result<(), String>,
        record: impl FnOnce(&crate::runtime::table::Table, &mut TableOverlay) -> Result<(), String>,
        transaction: Transaction,
    ) -> Result<(), String> {
        CALL_STACK.with(|s| {
            let mut stack = s.borrow_mut();
            let frame = stack.last_mut().ok_or("No active call frame")?;
            if frame.kind == CallFrameKind::Query {
                return Err("Writes are not allowed in query context".to_string());
            }
            validate(table, frame.overlays.get(table_name))?;
            let ov = frame
                .overlays
                .entry(table_name.to_string())
                .or_insert_with(|| TableOverlay::new(table.version()));
            record(table, ov)?;
            frame.transactions.push(transaction);
            Ok(())
        })
    }

    pub(crate) fn handle_insert_row(
        &self,
        caller_module_schema: &ModuleSchema,
        insert_row_request: InsertRowRequest,
    ) -> InsertRowResponse {
        let module_name = caller_module_schema.name.clone();
        let table_name = insert_row_request.table_name;
        let mut row = insert_row_request.row;
        if let Err(err) = self.ensure_current_frame_table_access(
            &ModuleSelection::Current,
            &table_name,
            TableAccessOp::Insert,
        ) {
            return InsertRowResponse::Err(err);
        }

        // Get module_arc from the active call frame (avoids modules.lock() HashMap lookup)
        let module_arc = match self.current_frame_module_arc() {
            Ok(m) => m,
            Err(e) => return InsertRowResponse::Err(e),
        };

        let tables = Self::read_tables(&module_arc);
        let table = match tables.get(&table_name) {
            Some(table) => table,
            None => return InsertRowResponse::Err("Table not found".into()),
        };
        let has_auto_inc = table.has_auto_inc();

        if let Err(detail) = table.schema.validate_row(&row, &caller_module_schema.type_definitions) {
            return InsertRowResponse::Err(format!(
                "Invalid row for table '{}' in module '{}': {}",
                table.schema.name, caller_module_schema.name, detail,
            ));
        }

        if has_auto_inc {
            // The auto-inc snapshot shares the table's atomic counters, so ids
            // handed out here never collide with concurrent reducers.
            let res = CALL_STACK.with(|s| {
                let mut stack = s.borrow_mut();
                let frame = stack.last_mut().ok_or("No active call frame".to_string())?;
                let snapshot = frame
                    .auto_inc_snapshots
                    .entry(table_name.clone())
                    .or_insert_with(|| table.auto_inc_snapshot());
                table
                    .apply_auto_inc_from_snapshot(&mut row, snapshot)
                    .map_err(|e| e.to_string())
            });
            if let Err(err) = res {
                return InsertRowResponse::Err(err);
            }
        }

        let pk = match IndexKey::try_from(&row.primary_key) {
            Ok(pk) => pk,
            Err(err) => return InsertRowResponse::Err(err),
        };
        let resp_row = has_auto_inc.then(|| row.clone());
        let staged_row = row.clone();
        let result = Self::stage_write(
            table,
            &table_name,
            |t, ov| t.ov_validate_insert(ov, &row).map_err(|e| e.to_string()),
            |t, ov| t.ov_record(ov, pk, Some(staged_row)).map_err(|e| e.to_string()),
            Transaction::Insert {
                module_name,
                table_name: table_name.clone(),
                new_row: row.clone(),
            },
        );
        match result {
            Ok(()) => InsertRowResponse::Ok(resp_row),
            Err(err) => InsertRowResponse::Err(err),
        }
    }

    pub(crate) fn handle_update_row(
        &self,
        caller_module_schema: &ModuleSchema,
        update_row_request: UpdateRowRequest,
    ) -> UpdateRowResponse {
        let module_name = caller_module_schema.name.clone();
        let table_name = update_row_request.table_name;
        let row = update_row_request.row;
        if let Err(err) = self.ensure_current_frame_table_access(
            &ModuleSelection::Current,
            &table_name,
            TableAccessOp::Update,
        ) {
            return UpdateRowResponse::Err(err);
        }
        let module_arc = match self.current_frame_module_arc() {
            Ok(m) => m,
            Err(e) => return UpdateRowResponse::Err(e),
        };
        let tables = Self::read_tables(&module_arc);
        let table = match tables.get(&table_name) {
            Some(table) => table,
            None => return UpdateRowResponse::Err("Table not found".into()),
        };
        // validate_row via the table's own schema — avoids a linear schema scan.
        if let Err(detail) = table
            .schema
            .validate_row(&row, &caller_module_schema.type_definitions)
        {
            return UpdateRowResponse::Err(format!(
                "Invalid row for table '{}' in module '{}': {}",
                table.schema.name, caller_module_schema.name, detail,
            ));
        }
        let pk = match IndexKey::try_from(&row.primary_key) {
            Ok(pk) => pk,
            Err(err) => return UpdateRowResponse::Err(err),
        };
        let staged_row = row.clone();
        let result = Self::stage_write(
            table,
            &table_name,
            |t, ov| t.ov_validate_update(ov, &row).map_err(|e| e.to_string()),
            |t, ov| t.ov_record(ov, pk, Some(staged_row)).map_err(|e| e.to_string()),
            Transaction::Update {
                module_name,
                table_name: table_name.clone(),
                update_row: row.clone(),
            },
        );
        match result {
            Ok(()) => UpdateRowResponse::Ok,
            Err(err) => UpdateRowResponse::Err(err),
        }
    }

    pub(crate) fn handle_delete_row(
        &self,
        caller_module_name: String,
        delete_row_request: DeleteRowRequest,
    ) -> DeleteRowResponse {
        let module_name = caller_module_name;
        let table_name = delete_row_request.table_name;
        let pk = delete_row_request.primary_key;
        if let Err(err) = self.ensure_current_frame_table_access(
            &ModuleSelection::Current,
            &table_name,
            TableAccessOp::Delete,
        ) {
            return DeleteRowResponse::Err(err);
        }
        let module_arc = match self.current_frame_module_arc() {
            Ok(m) => m,
            Err(e) => return DeleteRowResponse::Err(e),
        };
        let tables = Self::read_tables(&module_arc);
        let table = match tables.get(&table_name) {
            Some(table) => table,
            None => return DeleteRowResponse::Err("Table not found".into()),
        };
        let result = Self::stage_write(
            table,
            &table_name,
            |t, ov| t.ov_validate_delete(ov, &pk).map_err(|e| e.to_string()),
            |t, ov| t.ov_record(ov, pk.clone(), None).map_err(|e| e.to_string()),
            Transaction::Delete {
                module_name,
                table_name: table_name.clone(),
                deleted_row_id: pk.clone(),
            },
        );
        match result {
            Ok(()) => DeleteRowResponse::Ok,
            Err(err) => DeleteRowResponse::Err(err),
        }
    }

    pub(crate) fn handle_clear_table(
        &self,
        caller_module_name: String,
        request: ClearTableRequest,
    ) -> ClearTableResponse {
        if !matches!(request.module_selection, ModuleSelection::Current) {
            return ClearTableResponse::Err(
                "Cross-module table writes are not allowed; use ModuleSelection::Current"
                    .to_string(),
            );
        }
        let module_name = caller_module_name;
        if let Err(err) = self.ensure_current_frame_table_access(
            &request.module_selection,
            &request.table_name,
            TableAccessOp::Delete,
        ) {
            return ClearTableResponse::Err(err);
        }
        let module_arc = match self.current_frame_module_arc() {
            Ok(m) => m,
            Err(e) => return ClearTableResponse::Err(e),
        };
        let tables = Self::read_tables(&module_arc);
        let table = match tables.get(&request.table_name) {
            Some(table) => table,
            None => return ClearTableResponse::Err("Table not found".into()),
        };
        let result = Self::stage_write(
            table,
            &request.table_name,
            |_, _| Ok(()),
            |t, ov| {
                t.ov_record_clear(ov);
                Ok(())
            },
            Transaction::Clear {
                module_name,
                table_name: request.table_name.clone(),
            },
        );
        match result {
            Ok(()) => ClearTableResponse::Ok,
            Err(err) => ClearTableResponse::Err(err),
        }
    }

    /// Run `f` with the pending writes for `module_name.table_name`: those of
    /// the root reducer frame, which is the only frame that writes. Queries it
    /// calls therefore see its pending writes too (DESIGN §6.6).
    fn with_overlay<R>(
        module_name: &str,
        table_name: &str,
        f: impl FnOnce(Option<&TableOverlay>) -> R,
    ) -> R {
        CALL_STACK.with(|s| {
            let stack = s.borrow();
            let ov = stack
                .first()
                .filter(|frame| frame.kind == CallFrameKind::Reducer && frame.module == module_name)
                .and_then(|frame| frame.overlays.get(table_name));
            f(ov)
        })
    }

    /// Read access to `module`'s tables. Inside a query, the outermost query
    /// frame first pins the module with a read lock it keeps until it returns,
    /// so the query sees one consistent cut of every module it reads
    /// (DESIGN §6.6, Snapshots). Recursive reads never wait on other readers.
    fn read_tables(
        module: &Arc<Module>,
    ) -> parking_lot::RwLockReadGuard<'_, std::collections::HashMap<String, crate::runtime::table::Table>> {
        CALL_STACK.with(|s| {
            let mut stack = s.borrow_mut();
            if let Some(frame) = stack.iter_mut().find(|f| f.kind == CallFrameKind::Query) {
                if !frame.pins.contains_key(&module.schema.name) {
                    frame
                        .pins
                        .insert(module.schema.name.clone(), module.tables.read_arc_recursive());
                }
            }
        });
        module.tables.read_recursive()
    }

    /// Scan `table` of `module_name` with the calling frame's pending writes.
    pub(crate) fn frame_scan(
        module_name: &str,
        table_name: &str,
        table: &crate::runtime::table::Table,
    ) -> Result<Vec<interstice_abi::Row>, String> {
        Self::with_overlay(module_name, table_name, |ov| table.ov_scan(ov)).map_err(|e| e.to_string())
    }

    /// The module of the calling frame.
    fn reader_module(&self) -> String {
        CALL_STACK.with(|s| s.borrow().last().map(|f| f.module.clone())).unwrap_or_default()
    }

    /// Reading another module's data (DESIGN §7.5): its views, as this
    /// reader sees them; or its public tables. `Ok(Some(rows))` for a view,
    /// `Ok(None)` for a table it may read.
    fn published_read(&self, module: &Arc<crate::runtime::module::Module>, name: &str) -> Result<Option<Vec<interstice_abi::Row>>, String> {
        let reader = self.reader_module();
        if let Some(view) = module.schema.view(name) {
            if !Self::may_read_view_locally(&module.schema, name, &reader) {
                return Err(format!("view '{}.{name}' is internal", module.schema.name));
            }
            let viewer = self.local_viewer(&reader);
            return self.view_rows(module, view, &viewer, true).map(Some);
        }
        if reader != module.schema.name {
            if let Some(table) = module.schema.tables.iter().find(|t| t.name == name) {
                if table.visibility != interstice_abi::TableVisibility::Public {
                    return Err(format!(
                        "table '{}.{name}' is private; read one of its views",
                        module.schema.name
                    ));
                }
            }
        }
        Ok(None)
    }

    pub(crate) fn handle_table_scan(
        &self,
        table_scan_request: TableScanRequest,
    ) -> TableScanResponse {
        let module_name = match self.selected_module_name(&table_scan_request.module_selection) {
            Ok(module_name) => module_name,
            Err(err) => return TableScanResponse::Err(err),
        };
        if let Err(err) = self.ensure_current_frame_table_access(
            &table_scan_request.module_selection,
            &table_scan_request.table_name,
            TableAccessOp::Read,
        ) {
            return TableScanResponse::Err(err);
        }

        // For Current module, use the cached module_arc from the call frame
        let module_arc = match self.selected_module_arc(&table_scan_request.module_selection) {
            Ok(m) => m,
            Err(e) => return TableScanResponse::Err(e),
        };
        match self.published_read(&module_arc, &table_scan_request.table_name) {
            Ok(Some(rows)) => return TableScanResponse::Ok { rows },
            Ok(None) => {}
            Err(err) => return TableScanResponse::Err(err),
        }

        let tables = Self::read_tables(&module_arc);
        let table = match tables.get(&table_scan_request.table_name) {
            Some(table) => table,
            None => return TableScanResponse::Err("Table not found".into()),
        };

        match Self::with_overlay(&module_name, &table_scan_request.table_name, |ov| {
            table.ov_scan(ov)
        }) {
            Ok(rows) => TableScanResponse::Ok { rows },
            Err(err) => TableScanResponse::Err(err.to_string()),
        }
    }

    pub(crate) fn handle_table_get_by_primary_key(
        &self,
        request: TableGetByPrimaryKeyRequest,
    ) -> TableGetByPrimaryKeyResponse {
        let module_name = match self.selected_module_name(&request.module_selection) {
            Ok(module_name) => module_name,
            Err(err) => return TableGetByPrimaryKeyResponse::Err(err),
        };
        if let Err(err) = self.ensure_current_frame_table_access(
            &request.module_selection,
            &request.table_name,
            TableAccessOp::Read,
        ) {
            return TableGetByPrimaryKeyResponse::Err(err);
        }

        let module_arc = match self.selected_module_arc(&request.module_selection) {
            Ok(m) => m,
            Err(e) => return TableGetByPrimaryKeyResponse::Err(e),
        };
        match self.published_read(&module_arc, &request.table_name) {
            Ok(Some(rows)) => {
                return TableGetByPrimaryKeyResponse::Ok(
                    rows.into_iter().find(|r| interstice_abi::IndexKey::try_from(&r.primary_key).ok().as_ref() == Some(&request.primary_key)),
                );
            }
            Ok(None) => {}
            Err(err) => return TableGetByPrimaryKeyResponse::Err(err),
        }

        let row = {
            let tables = Self::read_tables(&module_arc);
            let table = match tables.get(&request.table_name) {
                Some(table) => table,
                None => return TableGetByPrimaryKeyResponse::Err("Table not found".into()),
            };
            Self::with_overlay(&module_name, &request.table_name, |ov| {
                table.ov_get(ov, &request.primary_key).cloned()
            })
        };

        TableGetByPrimaryKeyResponse::Ok(row)
    }

    pub(crate) fn handle_table_index_scan(
        &self,
        request: TableIndexScanRequest,
    ) -> TableIndexScanResponse {
        let module_name = match self.selected_module_name(&request.module_selection) {
            Ok(module_name) => module_name,
            Err(err) => return TableIndexScanResponse::Err(err),
        };
        if let Err(err) = self.ensure_current_frame_table_access(
            &request.module_selection,
            &request.table_name,
            TableAccessOp::Read,
        ) {
            return TableIndexScanResponse::Err(err);
        }

        let module_arc = match self.selected_module_arc(&request.module_selection) {
            Ok(m) => m,
            Err(e) => return TableIndexScanResponse::Err(e),
        };
        if module_arc.schema.view(&request.table_name).is_some() {
            return TableIndexScanResponse::Err("views have no indexes; scan the view".into());
        }
        if let Err(err) = self.published_read(&module_arc, &request.table_name) {
            return TableIndexScanResponse::Err(err);
        }

        let rows = {
            let tables = Self::read_tables(&module_arc);
            let table = match tables.get(&request.table_name) {
                Some(table) => table,
                None => return TableIndexScanResponse::Err("Table not found".into()),
            };

            Self::with_overlay(&module_name, &request.table_name, |ov| {
                table.ov_index_scan(ov, &request.field_name, &request.query)
            })
            .map_err(|err| err.to_string())
        };

        match rows {
            Ok(rows) => TableIndexScanResponse::Ok { rows },
            Err(err) => TableIndexScanResponse::Err(err),
        }
    }
}

fn op_label(op: &TableAccessOp) -> &'static str {
    match op {
        TableAccessOp::Read => "read",
        TableAccessOp::Insert => "insert",
        TableAccessOp::Update => "update",
        TableAccessOp::Delete => "delete",
    }
}

// ── History of logged tables (DESIGN §6.7) ───────────────────────────────────

impl Runtime {
    pub(crate) fn handle_history(
        &self,
        caller_module_name: String,
        call: interstice_abi::HistoryCall,
    ) -> interstice_abi::HistoryResponse {
        use interstice_abi::{HistoryCall, HistoryResponse};
        let result = match call {
            HistoryCall::Commits { table_name, limit } => self
                .history_schema(&table_name, &[TableAccessOp::Read])
                .and_then(|schema| {
                    self.persistence.history_commits(&caller_module_name, &schema.name, limit as usize)
                })
                .map(HistoryResponse::Commits),
            HistoryCall::RowsAt { table_name, point } => self
                .history_schema(&table_name, &[TableAccessOp::Read])
                .and_then(|schema| self.persistence.history_rows_at(&caller_module_name, &schema, point))
                .map(|(rows, _)| HistoryResponse::Rows(rows)),
            HistoryCall::Restore { table_name, point } => self
                .history_restore(caller_module_name, &table_name, point)
                .map(HistoryResponse::Restored),
        };
        result.unwrap_or_else(HistoryResponse::Err)
    }

    /// The schema of a logged table of the current module, after checking
    /// the frame may perform `ops` on it.
    fn history_schema(
        &self,
        table_name: &str,
        ops: &[TableAccessOp],
    ) -> Result<interstice_abi::TableSchema, String> {
        for op in ops {
            self.ensure_current_frame_table_access(&ModuleSelection::Current, table_name, *op)?;
        }
        let module_arc = self.current_frame_module_arc()?;
        let tables = Self::read_tables(&module_arc);
        let table = tables.get(table_name).ok_or("Table not found")?;
        if table.schema.persistence != interstice_abi::PersistenceKind::Logged {
            return Err(format!("table '{table_name}' is not logged, so it keeps no history"));
        }
        Ok(table.schema.clone())
    }

    /// Stage the writes that turn the table, as this reducer sees it, into
    /// its state at `point`, followed by the restore marker. All or nothing:
    /// nothing is staged if any write fails to validate.
    fn history_restore(
        &self,
        module_name: String,
        table_name: &str,
        point: interstice_abi::HistoryPoint,
    ) -> Result<u64, String> {
        let schema = self.history_schema(
            table_name,
            &[TableAccessOp::Insert, TableAccessOp::Update, TableAccessOp::Delete],
        )?;
        let (target_rows, to) = self.persistence.history_rows_at(&module_name, &schema, point)?;
        let mut target = std::collections::BTreeMap::new();
        for row in target_rows {
            target.insert(IndexKey::try_from(&row.primary_key)?, row);
        }
        let module_arc = self.current_frame_module_arc()?;
        let tables = Self::read_tables(&module_arc);
        let table = tables.get(table_name).ok_or("Table not found")?;

        CALL_STACK.with(|s| {
            let mut stack = s.borrow_mut();
            let frame = stack.last_mut().ok_or("No active call frame")?;
            if frame.kind == CallFrameKind::Query {
                return Err("Writes are not allowed in query context".to_string());
            }
            if frame.transactions.iter().any(
                |t| matches!(t, Transaction::Restore { table_name: name, .. } if name == table_name),
            ) {
                return Err(format!("table '{table_name}' was already restored in this transaction"));
            }
            let mut ov = frame
                .overlays
                .get(table_name)
                .cloned()
                .unwrap_or_else(|| TableOverlay::new(table.version()));
            let current = table.ov_scan(Some(&ov)).map_err(|e| e.to_string())?;

            // Diff by primary key: deletes, then updates, then inserts, each in
            // primary key order so the writes are deterministic.
            let mut deletes = Vec::new();
            let mut updates = Vec::new();
            for row in current {
                let pk = IndexKey::try_from(&row.primary_key)?;
                match target.remove(&pk) {
                    None => deletes.push(pk),
                    Some(new) => {
                        let same = interstice_abi::encode(&new).ok() == interstice_abi::encode(&row).ok();
                        if !same {
                            updates.push((pk, new));
                        }
                    }
                }
            }
            deletes.sort();
            updates.sort_by(|a, b| a.0.cmp(&b.0));

            let mut staged = Vec::new();
            let fail = |e: crate::IntersticeError| format!("restore of '{table_name}': {e}");
            for pk in deletes {
                table.ov_validate_delete(Some(&ov), &pk).map_err(fail)?;
                table.ov_record(&mut ov, pk.clone(), None).map_err(fail)?;
                staged.push(Transaction::Delete {
                    module_name: module_name.clone(),
                    table_name: table_name.to_string(),
                    deleted_row_id: pk,
                });
            }
            for (pk, row) in updates {
                table.ov_validate_update(Some(&ov), &row).map_err(fail)?;
                table.ov_record(&mut ov, pk, Some(row.clone())).map_err(fail)?;
                staged.push(Transaction::Update {
                    module_name: module_name.clone(),
                    table_name: table_name.to_string(),
                    update_row: row,
                });
            }
            for (pk, row) in target {
                table.ov_validate_insert(Some(&ov), &row).map_err(fail)?;
                table.ov_record(&mut ov, pk, Some(row.clone())).map_err(fail)?;
                staged.push(Transaction::Insert {
                    module_name: module_name.clone(),
                    table_name: table_name.to_string(),
                    new_row: row,
                });
            }
            staged.push(Transaction::Restore {
                module_name: module_name.clone(),
                table_name: table_name.to_string(),
                to,
            });
            // The overlay exists from here on, even for an empty diff, so a
            // concurrent commit to the table is caught at commit time.
            frame.overlays.insert(table_name.to_string(), ov);
            frame.transactions.extend(staged);
            Ok(to)
        })
    }
}
