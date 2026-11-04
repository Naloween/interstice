mod audio;
mod file;
mod gpu;
mod input;
mod module;
mod network;

pub use audio::*;
pub use file::*;
pub use gpu::*;
pub use input::*;
pub use module::*;
pub use network::*;

use crate::{IndexKey, IntersticeValue, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub enum HostCall {
    CallReducer(CallReducerRequest),
    Schedule(ScheduleRequest),
    CallQuery(CallQueryRequest),
    TableScan(TableScanRequest),
    TableGetByPrimaryKey(TableGetByPrimaryKeyRequest),
    TableIndexScan(TableIndexScanRequest),
    Gpu(GpuCall),
    Audio(AudioCall),
    File(FileCall),
    Module(ModuleCall),
    Network(NetworkCall),
    CurrentNodeId,
    History(HistoryCall),
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NodeSelection {
    Current,
    Other(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ModuleSelection {
    Current,
    Other(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ScheduleRequest {
    pub reducer_name: String,
    pub delay_ms: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ScheduleResponse {
    Ok,
    Err(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CallReducerRequest {
    pub node_selection: NodeSelection,
    pub module_selection: ModuleSelection,
    pub reducer_name: String,
    pub input: IntersticeValue,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum CallReducerResponse {
    Ok,
    Err(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CallQueryRequest {
    pub node_selection: NodeSelection,
    pub module_selection: ModuleSelection,
    pub query_name: String,
    pub input: IntersticeValue,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum CallQueryResponse {
    Ok(IntersticeValue),
    Err(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeterministicRandomRequest {}

#[derive(Debug, Serialize, Deserialize)]
pub enum DeterministicRandomResponse {
    Ok(u64),
    Err(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TimeRequest {}

#[derive(Debug, Serialize, Deserialize)]
pub enum TimeResponse {
    Ok { unix_ms: u64 },
    Err(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LogRequest {
    pub message: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InsertRowRequest {
    pub table_name: String,
    pub row: Row,
}

/// `Ok(None)` = row was inserted unchanged (no auto-inc fields); caller should use the row it
/// already has.  `Ok(Some(row))` = auto-inc fields were filled in; caller must use the returned
/// row.  This avoids serialising + deserialising the full row on the hot path for the common
/// non-auto-inc case.
#[derive(Debug, Deserialize, Serialize)]
pub enum InsertRowResponse {
    Ok(Option<Row>),
    Err(String),
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateRowRequest {
    pub table_name: String,
    pub row: Row,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum UpdateRowResponse {
    Ok,
    Err(String),
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DeleteRowRequest {
    pub table_name: String,
    pub primary_key: IndexKey,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum DeleteRowResponse {
    Ok,
    Err(String),
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ClearTableRequest {
    pub module_selection: ModuleSelection,
    pub table_name: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum ClearTableResponse {
    Ok,
    Err(String),
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TableScanRequest {
    pub module_selection: ModuleSelection,
    pub table_name: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum TableScanResponse {
    Ok { rows: Vec<Row> },
    Err(String),
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TableGetByPrimaryKeyRequest {
    pub module_selection: ModuleSelection,
    pub table_name: String,
    pub primary_key: IndexKey,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum TableGetByPrimaryKeyResponse {
    Ok(Option<Row>),
    Err(String),
}

#[derive(Debug, Deserialize, Serialize)]
pub enum IndexQuery {
    Eq(IndexKey),
    Lt(IndexKey),
    Lte(IndexKey),
    Gt(IndexKey),
    Gte(IndexKey),
    Range {
        min: IndexKey,
        max: IndexKey,
        include_min: bool,
        include_max: bool,
    },
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TableIndexScanRequest {
    pub module_selection: ModuleSelection,
    pub table_name: String,
    pub field_name: String,
    pub query: IndexQuery,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum TableIndexScanResponse {
    Ok { rows: Vec<Row> },
    Err(String),
}

/// A point of a logged table's history (DESIGN §6.7). The state at a point
/// is the table after every log entry the point covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum HistoryPoint {
    /// Entries with `tx <= id`; `id` need not be a commit of this table.
    Tx(u64),
    /// Entries with `timestamp_ms <= ms`.
    Time(u64),
    /// The `n`-th most recent commit of this table; `Back(0)` is now.
    Back(u32),
    /// The oldest retained state.
    Start,
}

/// One commit of a logged table: a run of log entries with the same `tx`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct HistoryCommit {
    pub tx: u64,
    pub timestamp_ms: u64,
    /// Row operations in the commit (the restore tag not counted).
    pub ops: u32,
    /// Set when the commit was a restore: the `tx` it restored to.
    pub restored_to: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum HistoryCall {
    /// The newest `limit` commits, newest first.
    Commits { table_name: String, limit: u32 },
    RowsAt { table_name: String, point: HistoryPoint },
    Restore { table_name: String, point: HistoryPoint },
}

#[derive(Debug, Deserialize, Serialize)]
pub enum HistoryResponse {
    Commits(Vec<HistoryCommit>),
    Rows(Vec<Row>),
    /// A restore: the `tx` of the last entry the point covers (`0`: empty start).
    Restored(u64),
    Err(String),
}

pub fn get_reducer_wrapper_name(reducer_name: &str) -> String {
    format!("__interstice_reducer_wrapper_{}", reducer_name)
}

pub fn get_query_wrapper_name(query_name: &str) -> String {
    format!("__interstice_query_wrapper_{}", query_name)
}
