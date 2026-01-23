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
    pub target_module: String,
    pub reducer: String,
    pub input: IntersticeValue,
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
    pub table_name: String,
    pub row: Row,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InsertRowResponse {}

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateRowRequest {
    pub table_name: String,
    pub key: IntersticeValue,
    pub row: Row,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateRowResponse {}

#[derive(Debug, Deserialize, Serialize)]
pub struct DeleteRowRequest {
    pub table_name: String,
    pub key: IntersticeValue,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DeleteRowResponse {}

#[derive(Debug, Deserialize, Serialize)]
pub struct TableScanRequest {
    pub table_name: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TableScanResponse {
    pub rows: Vec<Row>,
}
