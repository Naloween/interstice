use crate::{IntersticeValue, types::Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub enum HostCall {
    CallReducer(CallReducerRequest),
    Log(LogRequest),
    Abort(AbortRequest),
    InsertRow(InsertRowRequest),
    UpdateRow(UpdateRowRequest),
    DeleteRow(DeleteRowRequest),
    TableScan(TableScanRequest),
}

/// Host call: CALL_REDUCER
///
/// Semantics:
/// - Synchronous
/// - Stack-based
/// - Non-reentrant at reducer level
/// - Traps on error
///
/// Request: CallReducerRequest (postcard)
/// Response: PrimitiveValue (postcard)
///
/// Memory:
/// - Request buffer owned by caller
/// - Response buffer owned by host

#[derive(Debug, Serialize, Deserialize)]
pub struct CallReducerRequest {
    pub module_selection: ModuleSelection,
    pub reducer_name: String,
    pub input: IntersticeValue,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ModuleSelection {
    Current,
    Other(String),
}

pub type CallReducerResponse = IntersticeValue;

#[derive(Debug, Serialize, Deserialize)]
pub struct LogRequest {
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AbortRequest {
    pub message: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InsertRowRequest {
    pub module_selection: ModuleSelection,
    pub table_name: String,
    pub row: Row,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InsertRowResponse {}

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateRowRequest {
    pub module_selection: ModuleSelection,
    pub table_name: String,
    pub row: Row,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateRowResponse {}

#[derive(Debug, Deserialize, Serialize)]
pub struct DeleteRowRequest {
    pub module_selection: ModuleSelection,
    pub table_name: String,
    pub key: IntersticeValue,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DeleteRowResponse {}

#[derive(Debug, Deserialize, Serialize)]
pub struct TableScanRequest {
    pub module_selection: ModuleSelection,
    pub table_name: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TableScanResponse {
    pub rows: Vec<Row>,
}

pub fn get_reducer_wrapper_name(reducer_name: &str) -> String {
    format!("__interstice_reducer_wrapper_{}", reducer_name)
}
