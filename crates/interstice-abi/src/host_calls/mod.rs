mod audio;
mod file;
mod gpu;
mod input;
mod module;

pub use file::*;
pub use gpu::*;
pub use input::*;
pub use module::*;

use crate::{IntersticeValue, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub enum HostCall {
    CallReducer(CallReducerRequest),
    CallQuery(CallQueryRequest),
    Log(LogRequest),
    InsertRow(InsertRowRequest),
    UpdateRow(UpdateRowRequest),
    DeleteRow(DeleteRowRequest),
    TableScan(TableScanRequest),
    Gpu(GpuCall),
    Audio,
    Input,
    File(FileCall),
    Module(ModuleCall),
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
pub struct CallReducerRequest {
    pub node_selection: NodeSelection,
    pub module_selection: ModuleSelection,
    pub reducer_name: String,
    pub input: IntersticeValue,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CallQueryRequest {
    pub node_selection: NodeSelection,
    pub module_selection: ModuleSelection,
    pub query_name: String,
    pub input: IntersticeValue,
}

pub type CallQueryResponse = IntersticeValue;

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
    Ok,
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
    pub primary_key: IntersticeValue,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum DeleteRowResponse {
    Ok,
    Err(String),
}

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

pub fn get_query_wrapper_name(query_name: &str) -> String {
    format!("__interstice_query_wrapper_{}", query_name)
}
