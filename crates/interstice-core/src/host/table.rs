use interstice_abi::{
    DeleteRowRequest, DeleteRowResponse, InsertRowRequest, InsertRowResponse, TableScanRequest,
    TableScanResponse, UpdateRowRequest, UpdateRowResponse,
};

use crate::runtime::Runtime;

impl Runtime {
    pub(crate) fn handle_insert_row(
        &mut self,
        insert_row_request: InsertRowRequest,
    ) -> InsertRowResponse {
        let reducer_frame = self.call_stack.last_mut().unwrap();
        reducer_frame.transaction.inserts.push(insert_row_request);
        InsertRowResponse {}
    }
    pub(crate) fn handle_update_row(
        &mut self,
        update_row_request: UpdateRowRequest,
    ) -> UpdateRowResponse {
        let reducer_frame = self.call_stack.last_mut().unwrap();
        reducer_frame.transaction.updates.push(update_row_request);
        UpdateRowResponse {}
    }
    pub(crate) fn handle_delete_row(
        &mut self,
        delete_row_request: DeleteRowRequest,
    ) -> DeleteRowResponse {
        let reducer_frame = self.call_stack.last_mut().unwrap();
        reducer_frame.transaction.deletes.push(delete_row_request);
        DeleteRowResponse {}
    }
    pub(crate) fn handle_table_scan(
        &mut self,
        table_scan_request: TableScanRequest,
    ) -> TableScanResponse {
        self.modules
            .get(&self.call_stack.last().unwrap().module)
            .and_then(|module| module.tables.get(&table_scan_request.table))
            .map(|table| TableScanResponse {
                rows: table.rows.clone(),
            })
            .unwrap_or(TableScanResponse { rows: vec![] })
    }
}
