use serde::{Deserialize, Serialize};

use crate::interstice_abi_macros::IntersticeType;

#[derive(Debug, Serialize, Deserialize)]
pub enum FileCall {
    ReadFile(ReadFileRequest),
    WriteFile(WriteFileRequest),
    ListDir(ListDirRequest),
    Metadata(MetadataRequest),
    CreateDir(CreateDirRequest),
    RemoveFile(RemoveFileRequest),
    RemoveDir(RemoveDirRequest),
    Rename(RenameRequest),
    Copy(CopyRequest),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ReadFileRequest {
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ReadFileResponse {
    Ok(Vec<u8>),
    Err(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WriteFileRequest {
    pub path: String,
    pub data: Vec<u8>,
    pub append: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum WriteFileResponse {
    Ok,
    Err(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListDirRequest {
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ListDirResponse {
    Ok(Vec<DirEntry>),
    Err(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MetadataRequest {
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum MetadataResponse {
    Ok(FileMetadata),
    Err(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateDirRequest {
    pub path: String,
    pub recursive: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum CreateDirResponse {
    Ok,
    Err(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RemoveFileRequest {
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum RemoveFileResponse {
    Ok,
    Err(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RemoveDirRequest {
    pub path: String,
    pub recursive: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum RemoveDirResponse {
    Ok,
    Err(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RenameRequest {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum RenameResponse {
    Ok,
    Err(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CopyRequest {
    pub from: String,
    pub to: String,
    pub recursive: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum CopyResponse {
    Ok,
    Err(String),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DirEntry {
    pub name: String,
    pub path: String,
    pub file_type: FileType,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileMetadata {
    pub file_type: FileType,
    pub len: u64,
    pub readonly: bool,
    pub created: Option<u64>,
    pub modified: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum FileType {
    File,
    Directory,
    Symlink,
    Other,
}

#[derive(Debug, Deserialize, Serialize, IntersticeType, Clone)]
pub enum FileEvent {
    Created { path: String },
    Modified { path: String },
    Deleted { path: String },
    Renamed { from: String, to: String },
}
