use crate::{error::IntersticeError, runtime::Runtime, runtime::wasm::StoreState};
use interstice_abi::{
    CopyResponse, CreateDirResponse, DirEntry, FileCall, FileMetadata, FileType, ListDirResponse,
    MetadataResponse, ReadFileResponse, RemoveDirResponse, RemoveFileResponse, RenameResponse,
    WriteFileResponse,
};
use std::io::Write;
use wasmtime::{Caller, Memory};
impl Runtime {
    pub fn handle_file_call(
        &self,
        call: FileCall,
        memory: &Memory,
        caller: &mut Caller<'_, StoreState>,
    ) -> Result<Option<i64>, IntersticeError> {
        // The File authority is confined to the node's file root (DESIGN §7.4).
        let call = match self.confine_file_call(call) {
            Ok(call) => call,
            Err((call, err)) => return Ok(Some(self.reject_file_call(&call, err, memory, caller))),
        };
        let packed = match call {
            FileCall::ReadFile(req) => {
                let response = match std::fs::read(&req.path) {
                    Ok(bytes) => ReadFileResponse::Ok(bytes),
                    Err(err) => ReadFileResponse::Err(err.to_string()),
                };
                self.send_data_to_module(response, memory, caller)
            }
            FileCall::WriteFile(req) => {
                let result = if req.append {
                    std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&req.path)
                        .and_then(|mut file| file.write_all(&req.data))
                } else {
                    std::fs::write(&req.path, &req.data)
                };
                let response = match result {
                    Ok(()) => WriteFileResponse::Ok,
                    Err(err) => WriteFileResponse::Err(err.to_string()),
                };
                self.send_data_to_module(response, memory, caller)
            }
            FileCall::ListDir(req) => {
                let response = match std::fs::read_dir(&req.path) {
                    Ok(read_dir) => {
                        let mut entries = Vec::new();
                        for entry in read_dir.flatten() {
                            let path = entry.path();
                            let file_type = match entry.file_type() {
                                Ok(ft) if ft.is_dir() => FileType::Directory,
                                Ok(ft) if ft.is_file() => FileType::File,
                                Ok(ft) if ft.is_symlink() => FileType::Symlink,
                                Ok(_) => FileType::Other,
                                Err(_) => FileType::Other,
                            };
                            entries.push(DirEntry {
                                name: entry.file_name().to_string_lossy().to_string(),
                                path: path.to_string_lossy().to_string(),
                                file_type,
                            });
                        }
                        ListDirResponse::Ok(entries)
                    }
                    Err(err) => ListDirResponse::Err(err.to_string()),
                };
                self.send_data_to_module(response, memory, caller)
            }
            FileCall::Metadata(req) => {
                let response = match std::fs::metadata(&req.path) {
                    Ok(meta) => {
                        let file_type = if meta.is_dir() {
                            FileType::Directory
                        } else if meta.is_file() {
                            FileType::File
                        } else if meta.file_type().is_symlink() {
                            FileType::Symlink
                        } else {
                            FileType::Other
                        };

                        let created = meta
                            .created()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs());
                        let modified = meta
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs());

                        MetadataResponse::Ok(FileMetadata {
                            file_type,
                            len: meta.len(),
                            readonly: meta.permissions().readonly(),
                            created,
                            modified,
                        })
                    }
                    Err(err) => MetadataResponse::Err(err.to_string()),
                };
                self.send_data_to_module(response, memory, caller)
            }
            FileCall::CreateDir(req) => {
                let result = if req.recursive {
                    std::fs::create_dir_all(&req.path)
                } else {
                    std::fs::create_dir(&req.path)
                };
                let response = match result {
                    Ok(()) => CreateDirResponse::Ok,
                    Err(err) => CreateDirResponse::Err(err.to_string()),
                };
                self.send_data_to_module(response, memory, caller)
            }
            FileCall::RemoveFile(req) => {
                let response = match std::fs::remove_file(&req.path) {
                    Ok(()) => RemoveFileResponse::Ok,
                    Err(err) => RemoveFileResponse::Err(err.to_string()),
                };
                self.send_data_to_module(response, memory, caller)
            }
            FileCall::RemoveDir(req) => {
                let result = if req.recursive {
                    std::fs::remove_dir_all(&req.path)
                } else {
                    std::fs::remove_dir(&req.path)
                };
                let response = match result {
                    Ok(()) => RemoveDirResponse::Ok,
                    Err(err) => RemoveDirResponse::Err(err.to_string()),
                };
                self.send_data_to_module(response, memory, caller)
            }
            FileCall::Rename(req) => {
                let response = match std::fs::rename(&req.from, &req.to) {
                    Ok(()) => RenameResponse::Ok,
                    Err(err) => RenameResponse::Err(err.to_string()),
                };
                self.send_data_to_module(response, memory, caller)
            }
            FileCall::Copy(req) => {
                let result = if req.recursive {
                    copy_dir_all(&req.from, &req.to)
                } else {
                    std::fs::copy(&req.from, &req.to).map(|_| ())
                };
                let response = match result {
                    Ok(()) => CopyResponse::Ok,
                    Err(err) => CopyResponse::Err(err.to_string()),
                };
                self.send_data_to_module(response, memory, caller)
            }
        };

        Ok(Some(packed))
    }
}

impl Runtime {
    /// Every path of `call` resolved inside the file root.
    fn confine_file_call(&self, mut call: FileCall) -> Result<FileCall, (FileCall, String)> {
        let sec = &self.security;
        let fix = |p: &mut String| -> Result<(), String> {
            *p = sec.confine(p)?.to_string_lossy().into_owned();
            Ok(())
        };
        let result = match &mut call {
            FileCall::ReadFile(r) => fix(&mut r.path),
            FileCall::WriteFile(r) => fix(&mut r.path),
            FileCall::ListDir(r) => fix(&mut r.path),
            FileCall::Metadata(r) => fix(&mut r.path),
            FileCall::CreateDir(r) => fix(&mut r.path),
            FileCall::RemoveFile(r) => fix(&mut r.path),
            FileCall::RemoveDir(r) => fix(&mut r.path),
            FileCall::Rename(r) => fix(&mut r.from).and_then(|_| fix(&mut r.to)),
            FileCall::Copy(r) => fix(&mut r.from).and_then(|_| fix(&mut r.to)),
        };
        match result {
            Ok(()) => Ok(call),
            Err(err) => Err((call, err)),
        }
    }

    /// Answer `call` with an error, in the response type it expects.
    pub(crate) fn reject_file_call(
        &self,
        call: &FileCall,
        err: String,
        memory: &Memory,
        caller: &mut Caller<'_, StoreState>,
    ) -> i64 {
        match call {
            FileCall::ReadFile(_) => self.send_data_to_module(ReadFileResponse::Err(err), memory, caller),
            FileCall::WriteFile(_) => self.send_data_to_module(WriteFileResponse::Err(err), memory, caller),
            FileCall::ListDir(_) => self.send_data_to_module(ListDirResponse::Err(err), memory, caller),
            FileCall::Metadata(_) => self.send_data_to_module(MetadataResponse::Err(err), memory, caller),
            FileCall::CreateDir(_) => self.send_data_to_module(CreateDirResponse::Err(err), memory, caller),
            FileCall::RemoveFile(_) => self.send_data_to_module(RemoveFileResponse::Err(err), memory, caller),
            FileCall::RemoveDir(_) => self.send_data_to_module(RemoveDirResponse::Err(err), memory, caller),
            FileCall::Rename(_) => self.send_data_to_module(RenameResponse::Err(err), memory, caller),
            FileCall::Copy(_) => self.send_data_to_module(CopyResponse::Err(err), memory, caller),
        }
    }
}

fn copy_dir_all(from: &str, to: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let dest_path = std::path::Path::new(to).join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_all(
                entry.path().to_string_lossy().as_ref(),
                dest_path.to_string_lossy().as_ref(),
            )?;
        } else {
            std::fs::copy(entry.path(), dest_path)?;
        }
    }
    Ok(())
}
