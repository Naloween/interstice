use crate::host_calls::{host_call, unpack};
use interstice_abi::{
    CopyRequest, CopyResponse, CreateDirRequest, CreateDirResponse, DirEntry, FileCall,
    FileMetadata, HostCall, ListDirRequest, ListDirResponse, MetadataRequest, MetadataResponse,
    ReadFileRequest, ReadFileResponse, RemoveDirRequest, RemoveDirResponse, RemoveFileRequest,
    RemoveFileResponse, RenameRequest, RenameResponse, WriteFileRequest, WriteFileResponse,
};

pub fn read_file(path: String) -> Result<Vec<u8>, String> {
    let pack = host_call(HostCall::File(FileCall::ReadFile(ReadFileRequest { path })));
    let response: ReadFileResponse = unpack(pack);
    match response {
        ReadFileResponse::Ok(data) => Ok(data),
        ReadFileResponse::Err(err) => Err(err),
    }
}

pub fn write_file(path: String, data: Vec<u8>, append: bool) -> Result<(), String> {
    let pack = host_call(HostCall::File(FileCall::WriteFile(WriteFileRequest {
        path,
        data,
        append,
    })));
    let response: WriteFileResponse = unpack(pack);
    match response {
        WriteFileResponse::Ok => Ok(()),
        WriteFileResponse::Err(err) => Err(err),
    }
}

pub fn list_dir(path: String) -> Result<Vec<DirEntry>, String> {
    let pack = host_call(HostCall::File(FileCall::ListDir(ListDirRequest { path })));
    let response: ListDirResponse = unpack(pack);
    match response {
        ListDirResponse::Ok(entries) => Ok(entries),
        ListDirResponse::Err(err) => Err(err),
    }
}

pub fn metadata(path: String) -> Result<FileMetadata, String> {
    let pack = host_call(HostCall::File(FileCall::Metadata(MetadataRequest { path })));
    let response: MetadataResponse = unpack(pack);
    match response {
        MetadataResponse::Ok(meta) => Ok(meta),
        MetadataResponse::Err(err) => Err(err),
    }
}

pub fn create_dir(path: String, recursive: bool) -> Result<(), String> {
    let pack = host_call(HostCall::File(FileCall::CreateDir(CreateDirRequest {
        path,
        recursive,
    })));
    let response: CreateDirResponse = unpack(pack);
    match response {
        CreateDirResponse::Ok => Ok(()),
        CreateDirResponse::Err(err) => Err(err),
    }
}

pub fn remove_file(path: String) -> Result<(), String> {
    let pack = host_call(HostCall::File(FileCall::RemoveFile(RemoveFileRequest {
        path,
    })));
    let response: RemoveFileResponse = unpack(pack);
    match response {
        RemoveFileResponse::Ok => Ok(()),
        RemoveFileResponse::Err(err) => Err(err),
    }
}

pub fn remove_dir(path: String, recursive: bool) -> Result<(), String> {
    let pack = host_call(HostCall::File(FileCall::RemoveDir(RemoveDirRequest {
        path,
        recursive,
    })));
    let response: RemoveDirResponse = unpack(pack);
    match response {
        RemoveDirResponse::Ok => Ok(()),
        RemoveDirResponse::Err(err) => Err(err),
    }
}

pub fn rename(from: String, to: String) -> Result<(), String> {
    let pack = host_call(HostCall::File(FileCall::Rename(RenameRequest { from, to })));
    let response: RenameResponse = unpack(pack);
    match response {
        RenameResponse::Ok => Ok(()),
        RenameResponse::Err(err) => Err(err),
    }
}

pub fn copy(from: String, to: String, recursive: bool) -> Result<(), String> {
    let pack = host_call(HostCall::File(FileCall::Copy(CopyRequest {
        from,
        to,
        recursive,
    })));
    let response: CopyResponse = unpack(pack);
    match response {
        CopyResponse::Ok => Ok(()),
        CopyResponse::Err(err) => Err(err),
    }
}

pub struct File;

impl File {
    pub fn read(&self, path: String) -> Result<Vec<u8>, String> {
        read_file(path)
    }

    pub fn write(&self, path: String, data: Vec<u8>, append: bool) -> Result<(), String> {
        write_file(path, data, append)
    }

    pub fn list_dir(&self, path: String) -> Result<Vec<DirEntry>, String> {
        list_dir(path)
    }

    pub fn metadata(&self, path: String) -> Result<FileMetadata, String> {
        metadata(path)
    }

    pub fn create_dir(&self, path: String, recursive: bool) -> Result<(), String> {
        create_dir(path, recursive)
    }

    pub fn remove_file(&self, path: String) -> Result<(), String> {
        remove_file(path)
    }

    pub fn remove_dir(&self, path: String, recursive: bool) -> Result<(), String> {
        remove_dir(path, recursive)
    }

    pub fn rename(&self, from: String, to: String) -> Result<(), String> {
        rename(from, to)
    }

    pub fn copy(&self, from: String, to: String, recursive: bool) -> Result<(), String> {
        copy(from, to, recursive)
    }
}
