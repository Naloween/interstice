use crate::{
    runtime::transaction::Transaction,
    runtime::{Runtime, reducer::CallFrameKind},
};
use interstice_abi::{
    ClearTableRequest, ClearTableResponse, DeleteRowRequest, DeleteRowResponse, InsertRowRequest,
    InsertRowResponse, ModuleSchema, ModuleSelection, TableGetByPrimaryKeyRequest,
    TableGetByPrimaryKeyResponse, TableIndexScanRequest, TableIndexScanResponse, TableScanRequest,
    TableScanResponse, UpdateRowRequest, UpdateRowResponse,
};

impl Runtime {
    fn selected_module_name(&self, module_selection: &ModuleSelection) -> Result<String, String> {
        match module_selection {
            ModuleSelection::Current => self
                .call_stack
                .lock()
                .unwrap()
                .last()
                .map(|frame| frame.module.clone())
                .ok_or_else(|| "No active call frame".to_string()),
            ModuleSelection::Other(name) => Ok(name.clone()),
        }
    }

    pub(crate) fn handle_insert_row(
        &self,
        caller_module_schema: &ModuleSchema,
        insert_row_request: InsertRowRequest,
    ) -> InsertRowResponse {
        let module_name = caller_module_schema.name.clone();
        let mut row = insert_row_request.row;

        {
            let modules = self.modules.lock().unwrap();
            let module = match modules.get(&module_name) {
                Some(module) => module,
                None => return InsertRowResponse::Err("Module not found".into()),
            };
            let mut tables = module.tables.lock().unwrap();
            let table = match tables.get_mut(&insert_row_request.table_name) {
                Some(table) => table,
                None => return InsertRowResponse::Err("Table not found".into()),
            };

            let mut reducer_frame = self.call_stack.lock().unwrap();
            let reducer_frame = reducer_frame.last_mut().unwrap();
            let snapshot = reducer_frame
                .auto_inc_snapshots
                .entry(insert_row_request.table_name.clone())
                .or_insert_with(|| table.auto_inc_snapshot());

            if let Err(err) = table.apply_auto_inc_from_snapshot(&mut row, snapshot) {
                return InsertRowResponse::Err(err.to_string());
            }

            if let Err(err) = table.validate_insert(&row) {
                return InsertRowResponse::Err(err.to_string());
            }
        }

        if let Some(table) = caller_module_schema
            .tables
            .iter()
            .find(|t| t.name == insert_row_request.table_name)
        {
            if !table.validate_row(&row, &caller_module_schema.type_definitions) {
                return InsertRowResponse::Err(format!(
                    "Invalid row for table {} in module {}",
                    table.name.clone(),
                    caller_module_schema.name.clone(),
                ));
            }
        }

        let mut reducer_frame = self.call_stack.lock().unwrap();
        let reducer_frame = reducer_frame.last_mut().unwrap();
        if reducer_frame.kind == CallFrameKind::Query {
            return InsertRowResponse::Err("Insert not allowed in query context".into());
        }
        reducer_frame.transactions.push(Transaction::Insert {
            module_name,
            table_name: insert_row_request.table_name,
            new_row: row.clone(),
        });
        InsertRowResponse::Ok(row)
    }

    pub(crate) fn handle_update_row(
        &self,
        caller_module_schema: &ModuleSchema,
        update_row_request: UpdateRowRequest,
    ) -> UpdateRowResponse {
        let module_name = caller_module_schema.name.clone();
        if let Some(table) = caller_module_schema
            .tables
            .iter()
            .find(|t| t.name == update_row_request.table_name)
        {
            if !table.validate_row(
                &update_row_request.row,
                &caller_module_schema.type_definitions,
            ) {
                return UpdateRowResponse::Err(format!(
                    "Invalid row for table {} in module {}",
                    table.name.clone(),
                    caller_module_schema.name.clone()
                ));
            }
        }

        {
            let modules = self.modules.lock().unwrap();
            let module = match modules.get(&module_name) {
                Some(module) => module,
                None => return UpdateRowResponse::Err("Module not found".into()),
            };
            let tables = module.tables.lock().unwrap();
            let table = match tables.get(&update_row_request.table_name) {
                Some(table) => table,
                None => return UpdateRowResponse::Err("Table not found".into()),
            };
            if let Err(err) = table.validate_update(&update_row_request.row) {
                return UpdateRowResponse::Err(err.to_string());
            }
        }

        let mut reducer_frame = self.call_stack.lock().unwrap();
        let reducer_frame = reducer_frame.last_mut().unwrap();
        if reducer_frame.kind == CallFrameKind::Query {
            return UpdateRowResponse::Err("Update not allowed in query context".into());
        }
        reducer_frame.transactions.push(Transaction::Update {
            module_name,
            table_name: update_row_request.table_name,
            update_row: update_row_request.row,
        });
        UpdateRowResponse::Ok
    }
    pub(crate) fn handle_delete_row(
        &self,
        caller_module_name: String,
        delete_row_request: DeleteRowRequest,
    ) -> DeleteRowResponse {
        let module_name = caller_module_name;
        {
            let modules = self.modules.lock().unwrap();
            let module = match modules.get(&module_name) {
                Some(module) => module,
                None => return DeleteRowResponse::Err("Module not found".into()),
            };
            let tables = module.tables.lock().unwrap();
            let table = match tables.get(&delete_row_request.table_name) {
                Some(table) => table,
                None => return DeleteRowResponse::Err("Table not found".into()),
            };
            if let Err(err) = table.validate_delete(&delete_row_request.primary_key) {
                return DeleteRowResponse::Err(err.to_string());
            }
        }

        let mut reducer_frame = self.call_stack.lock().unwrap();
        let reducer_frame = reducer_frame.last_mut().unwrap();
        if reducer_frame.kind == CallFrameKind::Query {
            return DeleteRowResponse::Err("Delete not allowed in query context".into());
        }
        reducer_frame.transactions.push(Transaction::Delete {
            module_name,
            table_name: delete_row_request.table_name,
            deleted_row_id: delete_row_request.primary_key,
        });
        DeleteRowResponse::Ok
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
        {
            let modules = self.modules.lock().unwrap();
            let module = match modules.get(&module_name) {
                Some(module) => module,
                None => return ClearTableResponse::Err("Module not found".into()),
            };
            let tables = module.tables.lock().unwrap();
            if !tables.contains_key(&request.table_name) {
                return ClearTableResponse::Err("Table not found".into());
            }
        }

        let mut reducer_frame = self.call_stack.lock().unwrap();
        let reducer_frame = reducer_frame.last_mut().unwrap();
        if reducer_frame.kind == CallFrameKind::Query {
            return ClearTableResponse::Err("Clear not allowed in query context".into());
        }
        reducer_frame.transactions.push(Transaction::Clear {
            module_name,
            table_name: request.table_name,
        });
        ClearTableResponse::Ok
    }
    pub(crate) fn handle_table_scan(
        &self,
        table_scan_request: TableScanRequest,
    ) -> TableScanResponse {
        let module_name = match self.selected_module_name(&table_scan_request.module_selection) {
            Ok(module_name) => module_name,
            Err(err) => return TableScanResponse::Err(err),
        };

        let modules = self.modules.lock().unwrap();
        let module = match modules.get(&module_name) {
            Some(module) => module,
            None => return TableScanResponse::Err("Module not found".into()),
        };
        let tables = module.tables.lock().unwrap();
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
        let row = {
            let modules = self.modules.lock().unwrap();
            let module = match modules.get(&module_name) {
                Some(module) => module,
                None => return TableGetByPrimaryKeyResponse::Err("Module not found".into()),
            };
            let tables = module.tables.lock().unwrap();
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
        let rows = {
            let modules = self.modules.lock().unwrap();
            let module = match modules.get(&module_name) {
                Some(module) => module,
                None => return TableIndexScanResponse::Err("Module not found".into()),
            };
            let tables = module.tables.lock().unwrap();
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
