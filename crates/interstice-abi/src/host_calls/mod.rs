mod audio;
mod file;
mod gpu;
mod input;
mod module;

pub use audio::*;
pub use file::*;
pub use gpu::*;
pub use input::*;
pub use module::*;

use crate::{IndexKey, IntersticeValue, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub enum HostCall {
    CallReducer(CallReducerRequest),
    Schedule(ScheduleRequest),
    CallQuery(CallQueryRequest),
    DeterministicRandom(DeterministicRandomRequest),
    Time(TimeRequest),
    Log(LogRequest),
    InsertRow(InsertRowRequest),
    UpdateRow(UpdateRowRequest),
    DeleteRow(DeleteRowRequest),
    ClearTable(ClearTableRequest),
    TableScan(TableScanRequest),
    TableGetByPrimaryKey(TableGetByPrimaryKeyRequest),
    TableIndexScan(TableIndexScanRequest),
    Gpu(GpuCall),
    Audio(AudioCall),
    File(FileCall),
    Module(ModuleCall),
    CurrentNodeId,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum NodeSelection {
    Current,
    Other(String),
}

#[derive(Debug, Serialize, Deserialize)]
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
    pub module_selection: ModuleSelection,
    pub table_name: String,
    pub row: Row,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum InsertRowResponse {
    Ok(Row),
    Err(String),
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateRowRequest {
    pub module_selection: ModuleSelection,
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
    pub module_selection: ModuleSelection,
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

pub fn get_reducer_wrapper_name(reducer_name: &str) -> String {
    format!("__interstice_reducer_wrapper_{}", reducer_name)
}

pub fn get_query_wrapper_name(query_name: &str) -> String {
    format!("__interstice_query_wrapper_{}", query_name)
}
