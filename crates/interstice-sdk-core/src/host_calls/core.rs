use interstice_abi::{
    CallQueryRequest, CallReducerRequest, DeleteRowRequest, HostCall, IndexKey, InsertRowRequest,
    IntersticeValue, LogRequest, ModuleSelection, NodeSelection, Row, TableScanRequest,
    UpdateRowRequest, TableGetByPrimaryKeyRequest, TableGetByPrimaryKeyResponse,
    TableIndexScanRequest, TableIndexScanResponse, IndexQuery, InsertRowResponse,
};

pub fn log(message: &str) {
    let call = HostCall::Log(LogRequest {
        message: message.to_string(),
    });
    host_call(call);
}

pub fn call_reducer(
    node_selection: NodeSelection,
    module_selection: ModuleSelection,
    reducer_name: String,
    input: IntersticeValue,
) {
    let call = HostCall::CallReducer(CallReducerRequest {
        node_selection,
        module_selection,
        reducer_name,
        input,
    });

    host_call(call);
}

pub fn call_query(
    node_selection: NodeSelection,
    module_selection: ModuleSelection,
    query_name: String,
    input: IntersticeValue,
) -> IntersticeValue {
    let call = HostCall::CallQuery(CallQueryRequest {
        node_selection,
        module_selection,
        query_name,
        input,
    });

    let pack = host_call(call);
    let result: IntersticeValue = unpack(pack);
    return result;
}

pub fn insert_row(
    module_selection: ModuleSelection,
    table_name: String,
    row: Row,
) -> Result<Row, String> {
    let call = HostCall::InsertRow(InsertRowRequest {
        module_selection,
        table_name,
        row,
    });

    let pack = host_call(call);
    let response: InsertRowResponse = unpack(pack);
    match response {
        InsertRowResponse::Ok(row) => Ok(row),
        InsertRowResponse::Err(err) => Err(err),
    }
}

pub fn update_row(module_selection: ModuleSelection, table_name: String, row: Row) {
    let call = HostCall::UpdateRow(UpdateRowRequest {
        module_selection,
        table_name,
        row,
    });

    host_call(call);
}

pub fn delete_row(module_selection: ModuleSelection, table_name: String, primary_key: IndexKey) {
    let call = HostCall::DeleteRow(DeleteRowRequest {
        module_selection,
        table_name,
        primary_key,
    });

    host_call(call);
}

pub fn scan(module_selection: ModuleSelection, table_name: String) -> Vec<Row> {
    let call = HostCall::TableScan(TableScanRequest {
        module_selection,
        table_name,
    });

    let pack = host_call(call);
    let rows: Vec<Row> = unpack(pack);
    return rows;
}

pub fn get_by_primary_key(
    module_selection: ModuleSelection,
    table_name: String,
    primary_key: IndexKey,
) -> Result<Option<Row>, String> {
    let call = HostCall::TableGetByPrimaryKey(TableGetByPrimaryKeyRequest {
        module_selection,
        table_name,
        primary_key,
    });

    let pack = host_call(call);
    let response: TableGetByPrimaryKeyResponse = unpack(pack);
    match response {
        TableGetByPrimaryKeyResponse::Ok(row) => Ok(row),
        TableGetByPrimaryKeyResponse::Err(err) => Err(err),
    }
}

pub fn scan_index(
    module_selection: ModuleSelection,
    table_name: String,
    field_name: String,
    query: IndexQuery,
) -> Result<Vec<Row>, String> {
    let call = HostCall::TableIndexScan(TableIndexScanRequest {
        module_selection,
        table_name,
        field_name,
        query,
    });

    let pack = host_call(call);
    let response: TableIndexScanResponse = unpack(pack);
    match response {
        TableIndexScanResponse::Ok { rows } => Ok(rows),
        TableIndexScanResponse::Err(err) => Err(err),
    }
}

use interstice_abi::{QueryContext, ReducerContext};

use crate::host_calls::{host_call, unpack};

pub trait HostLog {
    fn log(&self, message: &str);
}

impl HostLog for ReducerContext {
    fn log(&self, message: &str) {
        log(message);
    }
}

impl HostLog for QueryContext {
    fn log(&self, message: &str) {
        log(message);
    }
}
