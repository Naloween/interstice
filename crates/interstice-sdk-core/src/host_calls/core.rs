use interstice_abi::{
    CallQueryRequest, CallQueryResponse, CallReducerRequest, CallReducerResponse,
    ClearTableRequest, ClearTableResponse, DeleteRowRequest, DeleteRowResponse,
    DeterministicRandomRequest, DeterministicRandomResponse, HostCall, IndexKey, IndexQuery,
    InsertRowRequest, InsertRowResponse, IntersticeValue, LogRequest, ModuleSelection,
    NodeSelection, Row, TableGetByPrimaryKeyRequest, TableGetByPrimaryKeyResponse,
    TableIndexScanRequest, TableIndexScanResponse, TableScanRequest, TableScanResponse,
    TimeRequest, TimeResponse, UpdateRowRequest, UpdateRowResponse,
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
) -> Result<(), String> {
    let call = HostCall::CallReducer(CallReducerRequest {
        node_selection,
        module_selection,
        reducer_name,
        input,
    });

    let pack = host_call(call);
    let response: CallReducerResponse = unpack(pack);
    match response {
        CallReducerResponse::Ok => Ok(()),
        CallReducerResponse::Err(err) => Err(err),
    }
}

pub fn call_query(
    node_selection: NodeSelection,
    module_selection: ModuleSelection,
    query_name: String,
    input: IntersticeValue,
) -> Result<IntersticeValue, String> {
    let call = HostCall::CallQuery(CallQueryRequest {
        node_selection,
        module_selection,
        query_name,
        input,
    });

    let pack = host_call(call);
    let response: CallQueryResponse = unpack(pack);
    match response {
        CallQueryResponse::Ok(value) => Ok(value),
        CallQueryResponse::Err(err) => Err(err),
    }
}

pub fn deterministic_random_u64() -> Result<u64, String> {
    let call = HostCall::DeterministicRandom(DeterministicRandomRequest {});
    let pack = host_call(call);
    let response: DeterministicRandomResponse = unpack(pack);
    match response {
        DeterministicRandomResponse::Ok(value) => Ok(value),
        DeterministicRandomResponse::Err(err) => Err(err),
    }
}

pub fn time_now_ms() -> Result<u64, String> {
    let call = HostCall::Time(TimeRequest {});
    let pack = host_call(call);
    let response: TimeResponse = unpack(pack);
    match response {
        TimeResponse::Ok { unix_ms } => Ok(unix_ms),
        TimeResponse::Err(err) => Err(err),
    }
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

pub fn update_row(
    module_selection: ModuleSelection,
    table_name: String,
    row: Row,
) -> Result<(), String> {
    let call = HostCall::UpdateRow(UpdateRowRequest {
        module_selection,
        table_name,
        row,
    });

    let pack = host_call(call);
    let response: UpdateRowResponse = unpack(pack);
    match response {
        UpdateRowResponse::Ok => Ok(()),
        UpdateRowResponse::Err(err) => Err(err),
    }
}

pub fn delete_row(
    module_selection: ModuleSelection,
    table_name: String,
    primary_key: IndexKey,
) -> Result<(), String> {
    let call = HostCall::DeleteRow(DeleteRowRequest {
        module_selection,
        table_name,
        primary_key,
    });

    let pack = host_call(call);
    let response: DeleteRowResponse = unpack(pack);
    match response {
        DeleteRowResponse::Ok => Ok(()),
        DeleteRowResponse::Err(err) => Err(err),
    }
}

pub fn clear_table(module_selection: ModuleSelection, table_name: String) -> Result<(), String> {
    let call = HostCall::ClearTable(ClearTableRequest {
        module_selection,
        table_name,
    });

    let pack = host_call(call);
    let response: ClearTableResponse = unpack(pack);
    match response {
        ClearTableResponse::Ok => Ok(()),
        ClearTableResponse::Err(err) => Err(err),
    }
}

pub fn scan(module_selection: ModuleSelection, table_name: String) -> Result<Vec<Row>, String> {
    let call = HostCall::TableScan(TableScanRequest {
        module_selection,
        table_name,
    });

    let pack = host_call(call);
    let response: TableScanResponse = unpack(pack);
    match response {
        TableScanResponse::Ok { rows } => Ok(rows),
        TableScanResponse::Err(err) => Err(err),
    }
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

pub trait HostTime {
    fn time_now_ms(&self) -> Result<u64, String>;
}

pub trait HostDeterministicRandom {
    fn deterministic_random_u64(&self) -> Result<u64, String>;
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

impl HostTime for ReducerContext {
    fn time_now_ms(&self) -> Result<u64, String> {
        time_now_ms()
    }
}

impl HostTime for QueryContext {
    fn time_now_ms(&self) -> Result<u64, String> {
        time_now_ms()
    }
}

impl HostDeterministicRandom for ReducerContext {
    fn deterministic_random_u64(&self) -> Result<u64, String> {
        deterministic_random_u64()
    }
}

impl HostDeterministicRandom for QueryContext {
    fn deterministic_random_u64(&self) -> Result<u64, String> {
        deterministic_random_u64()
    }
}
