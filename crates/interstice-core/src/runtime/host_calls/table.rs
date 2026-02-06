use crate::{runtime::Runtime, runtime::transaction::Transaction};
use interstice_abi::{
    DeleteRowRequest, DeleteRowResponse, InsertRowRequest, InsertRowResponse, ModuleSchema,
    TableScanRequest, TableScanResponse, UpdateRowRequest, UpdateRowResponse,
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
        reducer_frame.transactions.push(Transaction::Delete {
            module_name: caller_module_name,
            table_name: delete_row_request.table_name,
            deleted_row_id: delete_row_request.key,
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
                    .map(|t| t.rows.clone())
            })
            .map(|rows| TableScanResponse { rows })
            .unwrap_or(TableScanResponse { rows: vec![] })
    }
}
