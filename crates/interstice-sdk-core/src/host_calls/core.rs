use interstice_abi::{
    CallQueryRequest, CallReducerRequest, HostCall, InsertRowRequest, IntersticeValue, LogRequest,
    ModuleSelection, NodeSelection, Row, TableScanRequest,
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

pub fn insert_row(module_selection: ModuleSelection, table_name: String, row: Row) {
    let call = HostCall::InsertRow(InsertRowRequest {
        module_selection,
        table_name,
        row,
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
