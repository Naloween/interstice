use interstice_abi::{
    DeleteRowRequest, DeleteRowResponse, InsertRowRequest, InsertRowResponse, TableScanRequest,
    TableScanResponse, UpdateRowRequest, UpdateRowResponse,
};

use crate::runtime::{Runtime, transaction::Transaction};

impl Runtime {
    pub(crate) fn handle_insert_row(
        &mut self,
        caller_module_name: String,
        insert_row_request: InsertRowRequest,
    ) -> InsertRowResponse {
        let reducer_frame = self.call_stack.last_mut().unwrap();
        reducer_frame.transactions.push(Transaction::Insert {
            module_name: caller_module_name,
            table_name: insert_row_request.table_name,
            new_row: insert_row_request.row,
        });
        InsertRowResponse {}
    }
    pub(crate) fn handle_update_row(
        &mut self,
        caller_module_name: String,
        update_row_request: UpdateRowRequest,
    ) -> UpdateRowResponse {
        let reducer_frame = self.call_stack.last_mut().unwrap();
        reducer_frame.transactions.push(Transaction::Update {
            module_name: caller_module_name,
            table_name: update_row_request.table_name,
            update_row: update_row_request.row,
        });
        UpdateRowResponse {}
    }
    pub(crate) fn handle_delete_row(
        &mut self,
        caller_module_name: String,
        delete_row_request: DeleteRowRequest,
    ) -> DeleteRowResponse {
        let reducer_frame = self.call_stack.last_mut().unwrap();
        reducer_frame.transactions.push(Transaction::Delete {
            module_name: caller_module_name,
            table_name: delete_row_request.table_name,
            deleted_row_id: delete_row_request.key,
        });
        DeleteRowResponse {}
    }
    pub(crate) fn handle_table_scan(
        &mut self,
        table_scan_request: TableScanRequest,
    ) -> TableScanResponse {
        self.modules
            .get(&self.call_stack.last().unwrap().module)
            .and_then(|module| module.tables.get(&table_scan_request.table_name))
            .map(|table| TableScanResponse {
                rows: table.rows.clone(),
            })
            .unwrap_or(TableScanResponse { rows: vec![] })
    }
}
