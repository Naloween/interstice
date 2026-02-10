use crate::{
    runtime::transaction::Transaction,
    runtime::{Runtime, reducer::CallFrameKind},
};
use interstice_abi::{
    DeleteRowRequest, DeleteRowResponse, InsertRowRequest, InsertRowResponse, ModuleSchema,
    TableGetByPrimaryKeyRequest, TableGetByPrimaryKeyResponse, TableIndexScanRequest,
    TableIndexScanResponse, TableScanRequest, TableScanResponse, UpdateRowRequest,
    UpdateRowResponse,
};

impl Runtime {
    pub(crate) fn handle_insert_row(
        &self,
        caller_module_schema: &ModuleSchema,
        insert_row_request: InsertRowRequest,
    ) -> InsertRowResponse {
        if let Some(table) = caller_module_schema
            .tables
            .iter()
            .find(|t| t.name == insert_row_request.table_name)
        {
            if !table.validate_row(
                &insert_row_request.row,
                &caller_module_schema.type_definitions,
            ) {
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
            module_name: caller_module_schema.name.clone(),
            table_name: insert_row_request.table_name,
            new_row: insert_row_request.row,
        });
        InsertRowResponse::Ok
    }

    pub(crate) fn handle_update_row(
        &self,
        caller_module_name: String,
        update_row_request: UpdateRowRequest,
    ) -> UpdateRowResponse {
        let mut reducer_frame = self.call_stack.lock().unwrap();
        let reducer_frame = reducer_frame.last_mut().unwrap();
        if reducer_frame.kind == CallFrameKind::Query {
            return UpdateRowResponse::Err("Update not allowed in query context".into());
        }
        reducer_frame.transactions.push(Transaction::Update {
            module_name: caller_module_name,
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
        let mut reducer_frame = self.call_stack.lock().unwrap();
        let reducer_frame = reducer_frame.last_mut().unwrap();
        if reducer_frame.kind == CallFrameKind::Query {
            return DeleteRowResponse::Err("Delete not allowed in query context".into());
        }
        reducer_frame.transactions.push(Transaction::Delete {
            module_name: caller_module_name,
            table_name: delete_row_request.table_name,
            deleted_row_id: delete_row_request.primary_key,
        });
        DeleteRowResponse::Ok
    }
    pub(crate) fn handle_table_scan(
        &self,
        table_scan_request: TableScanRequest,
    ) -> TableScanResponse {
        self.modules
            .lock()
            .unwrap()
            .get(&self.call_stack.lock().unwrap().last().unwrap().module)
            .and_then(|module| {
                module
                    .tables
                    .lock()
                    .unwrap()
                    .get(&table_scan_request.table_name)
                    .map(|t| t.scan().to_vec())
            })
            .map(|rows| TableScanResponse { rows })
            .unwrap_or(TableScanResponse { rows: vec![] })
    }

    pub(crate) fn handle_table_get_by_primary_key(
        &self,
        request: TableGetByPrimaryKeyRequest,
    ) -> TableGetByPrimaryKeyResponse {
        let row = {
            let modules = self.modules.lock().unwrap();
            let module = match modules.get(&self.call_stack.lock().unwrap().last().unwrap().module)
            {
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
        let rows = {
            let modules = self.modules.lock().unwrap();
            let module = match modules.get(&self.call_stack.lock().unwrap().last().unwrap().module)
            {
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
