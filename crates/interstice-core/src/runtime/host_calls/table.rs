use crate::{
    runtime::transaction::Transaction,
    runtime::{Runtime, module::Module, reducer::{CallFrameKind, CALL_STACK}},
};
use interstice_abi::{
    ClearTableRequest, ClearTableResponse, DeleteRowRequest, DeleteRowResponse, InsertRowRequest,
    InsertRowResponse, ModuleSchema, ModuleSelection, NodeSelection, ReducerTableRef,
    TableGetByPrimaryKeyRequest, TableGetByPrimaryKeyResponse, TableIndexScanRequest,
    TableIndexScanResponse, TableScanRequest, TableScanResponse, UpdateRowRequest, UpdateRowResponse,
};
use std::sync::Arc;

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
                Err(format!(
                    "Reducer '{}.{}' lacks {} permission for {} (declared access uses structured node.module.table refs)",
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

    pub(crate) fn handle_insert_row(
        &self,
        caller_module_schema: &ModuleSchema,
        insert_row_request: InsertRowRequest,
    ) -> InsertRowResponse {
        let module_name = caller_module_schema.name.clone();
        let mut row = insert_row_request.row;
        if let Err(err) = self.ensure_current_frame_table_access(
            &ModuleSelection::Current,
            &insert_row_request.table_name,
            TableAccessOp::Insert,
        ) {
            return InsertRowResponse::Err(err);
        }

        // Get module_arc from the active call frame (avoids modules.lock() HashMap lookup)
        let module_arc = match self.current_frame_module_arc() {
            Ok(m) => m,
            Err(e) => return InsertRowResponse::Err(e),
        };

        let has_auto_inc;
        {
            let mut tables = module_arc.tables.lock();
            let table = match tables.get_mut(&insert_row_request.table_name) {
                Some(table) => table,
                None => return InsertRowResponse::Err("Table not found".into()),
            };

            has_auto_inc = table.has_auto_inc();

            if let Err(detail) = table.schema.validate_row(&row, &caller_module_schema.type_definitions) {
                return InsertRowResponse::Err(format!(
                    "Invalid row for table '{}' in module '{}': {}",
                    table.schema.name, caller_module_schema.name, detail,
                ));
            }

            // Initialise auto-inc snapshot if this is the first insert into this table
            // in the current reducer call (snapshot creation needs &table, done outside TLS borrow).
            let needs_snapshot = CALL_STACK.with(|s| {
                s.borrow()
                    .last()
                    .map_or(true, |f| !f.auto_inc_snapshots.contains_key(&insert_row_request.table_name))
            });
            if needs_snapshot {
                let snapshot = table.auto_inc_snapshot();
                CALL_STACK.with(|s| {
                    if let Some(f) = s.borrow_mut().last_mut() {
                        f.auto_inc_snapshots.insert(insert_row_request.table_name.clone(), snapshot);
                    }
                });
            }

            if let Err(err) = CALL_STACK.with(|s| {
                let mut stack = s.borrow_mut();
                let frame = stack.last_mut().unwrap();
                let snapshot = frame.auto_inc_snapshots.get(&insert_row_request.table_name).unwrap();
                table.apply_auto_inc_from_snapshot(&mut row, snapshot)
            }) {
                return InsertRowResponse::Err(err.to_string());
            }

            if let Err(err) = table.validate_insert(&row) {
                return InsertRowResponse::Err(err.to_string());
            }
        }

        CALL_STACK.with(|s| {
            let mut stack = s.borrow_mut();
            let reducer_frame = stack.last_mut().unwrap();
            if reducer_frame.kind == CallFrameKind::Query {
                return InsertRowResponse::Err("Insert not allowed in query context".into());
            }

            if has_auto_inc {
                let resp_row = row.clone();
                reducer_frame.transactions.push(Transaction::Insert {
                    module_name,
                    table_name: insert_row_request.table_name,
                    new_row: row,
                });
                InsertRowResponse::Ok(Some(resp_row))
            } else {
                reducer_frame.transactions.push(Transaction::Insert {
                    module_name,
                    table_name: insert_row_request.table_name,
                    new_row: row,
                });
                InsertRowResponse::Ok(None)
            }
        })
    }

    pub(crate) fn handle_update_row(
        &self,
        caller_module_schema: &ModuleSchema,
        update_row_request: UpdateRowRequest,
    ) -> UpdateRowResponse {
        let module_name = caller_module_schema.name.clone();
        if let Err(err) = self.ensure_current_frame_table_access(
            &ModuleSelection::Current,
            &update_row_request.table_name,
            TableAccessOp::Update,
        ) {
            return UpdateRowResponse::Err(err);
        }
        {
            let module_arc = match self.current_frame_module_arc() {
                Ok(m) => m,
                Err(e) => return UpdateRowResponse::Err(e),
            };
            let tables = module_arc.tables.lock();
            let table = match tables.get(&update_row_request.table_name) {
                Some(table) => table,
                None => return UpdateRowResponse::Err("Table not found".into()),
            };
            // validate_row via the table's own schema — avoids a linear schema scan.
            if let Err(detail) = table.schema.validate_row(
                &update_row_request.row,
                &caller_module_schema.type_definitions,
            ) {
                return UpdateRowResponse::Err(format!(
                    "Invalid row for table '{}' in module '{}': {}",
                    table.schema.name, caller_module_schema.name, detail,
                ));
            }
            if let Err(err) = table.validate_update(&update_row_request.row) {
                return UpdateRowResponse::Err(err.to_string());
            }
        }

        CALL_STACK.with(|s| {
            let mut stack = s.borrow_mut();
            let reducer_frame = stack.last_mut().unwrap();
            if reducer_frame.kind == CallFrameKind::Query {
                return UpdateRowResponse::Err("Update not allowed in query context".into());
            }
            reducer_frame.transactions.push(Transaction::Update {
                module_name,
                table_name: update_row_request.table_name,
                update_row: update_row_request.row,
            });
            UpdateRowResponse::Ok
        })
    }

    pub(crate) fn handle_delete_row(
        &self,
        caller_module_name: String,
        delete_row_request: DeleteRowRequest,
    ) -> DeleteRowResponse {
        let module_name = caller_module_name;
        if let Err(err) = self.ensure_current_frame_table_access(
            &ModuleSelection::Current,
            &delete_row_request.table_name,
            TableAccessOp::Delete,
        ) {
            return DeleteRowResponse::Err(err);
        }
        {
            let module_arc = match self.current_frame_module_arc() {
                Ok(m) => m,
                Err(e) => return DeleteRowResponse::Err(e),
            };
            let tables = module_arc.tables.lock();
            let table = match tables.get(&delete_row_request.table_name) {
                Some(table) => table,
                None => return DeleteRowResponse::Err("Table not found".into()),
            };
            if let Err(err) = table.validate_delete(&delete_row_request.primary_key) {
                return DeleteRowResponse::Err(err.to_string());
            }
        }

        CALL_STACK.with(|s| {
            let mut stack = s.borrow_mut();
            let reducer_frame = stack.last_mut().unwrap();
            if reducer_frame.kind == CallFrameKind::Query {
                return DeleteRowResponse::Err("Delete not allowed in query context".into());
            }
            reducer_frame.transactions.push(Transaction::Delete {
                module_name,
                table_name: delete_row_request.table_name,
                deleted_row_id: delete_row_request.primary_key,
            });
            DeleteRowResponse::Ok
        })
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
        {
            let module_arc = match self.current_frame_module_arc() {
                Ok(m) => m,
                Err(e) => return ClearTableResponse::Err(e),
            };
            let tables = module_arc.tables.lock();
            if !tables.contains_key(&request.table_name) {
                return ClearTableResponse::Err("Table not found".into());
            }
        }

        CALL_STACK.with(|s| {
            let mut stack = s.borrow_mut();
            let reducer_frame = stack.last_mut().unwrap();
            if reducer_frame.kind == CallFrameKind::Query {
                return ClearTableResponse::Err("Clear not allowed in query context".into());
            }
            reducer_frame.transactions.push(Transaction::Clear {
                module_name,
                table_name: request.table_name,
            });
            ClearTableResponse::Ok
        })
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
        let module_arc = match &table_scan_request.module_selection {
            ModuleSelection::Current => match self.current_frame_module_arc() {
                Ok(m) => m,
                Err(e) => return TableScanResponse::Err(e),
            },
            ModuleSelection::Other(_) => {
                let modules = self.modules.lock();
                match modules.get(&module_name) {
                    Some(m) => m.clone(),
                    None => return TableScanResponse::Err("Module not found".into()),
                }
            }
        };

        let tables = module_arc.tables.lock();
        let table = match tables.get(&table_scan_request.table_name) {
            Some(table) => table,
            None => return TableScanResponse::Err("Table not found".into()),
        };

        TableScanResponse::Ok {
            rows: table.scan().to_vec(),
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

        let module_arc = match &request.module_selection {
            ModuleSelection::Current => match self.current_frame_module_arc() {
                Ok(m) => m,
                Err(e) => return TableGetByPrimaryKeyResponse::Err(e),
            },
            ModuleSelection::Other(_) => {
                let modules = self.modules.lock();
                match modules.get(&module_name) {
                    Some(m) => m.clone(),
                    None => return TableGetByPrimaryKeyResponse::Err("Module not found".into()),
                }
            }
        };

        let row = {
            let tables = module_arc.tables.lock();
            let table = match tables.get(&request.table_name) {
                Some(table) => table,
                None => return TableGetByPrimaryKeyResponse::Err("Table not found".into()),
            };
            table.get_by_primary_key(&request.primary_key).cloned()
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

        let module_arc = match &request.module_selection {
            ModuleSelection::Current => match self.current_frame_module_arc() {
                Ok(m) => m,
                Err(e) => return TableIndexScanResponse::Err(e),
            },
            ModuleSelection::Other(_) => {
                let modules = self.modules.lock();
                match modules.get(&module_name) {
                    Some(m) => m.clone(),
                    None => return TableIndexScanResponse::Err("Module not found".into()),
                }
            }
        };

        let rows = {
            let tables = module_arc.tables.lock();
            let table = match tables.get(&request.table_name) {
                Some(table) => table,
                None => return TableIndexScanResponse::Err("Table not found".into()),
            };

            match table.get_by_index(&request.field_name, &request.query) {
                Ok(rows) => Ok(rows.into_iter().cloned().collect::<Vec<_>>()),
                Err(err) => Err(err.to_string()),
            }
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
