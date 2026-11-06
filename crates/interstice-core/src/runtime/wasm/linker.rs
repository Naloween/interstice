use super::StoreState;
use crate::logger::{LogLevel, LogSource};
use interstice_abi::{IndexKey, InsertRowRequest, ModuleSelection, Row, TableGetByPrimaryKeyRequest, decode, encode};
use wasmtime::{Caller, Linker};

/// Read at most `max` bytes starting at WASM linear memory address `wasm_ptr` into a caller-
/// supplied stack buffer.  Returns a borrowed `&str` on success, or `None` on error.
/// Avoids heap allocation for short strings (table names, field names).
#[inline]
fn read_str_stack<'a>(
    caller: &mut Caller<'_, StoreState>,
    memory: &wasmtime::Memory,
    wasm_ptr: i32,
    wasm_len: i32,
    buf: &'a mut [u8],
) -> Option<&'a str> {
    let len = wasm_len.max(0) as usize;
    if len > buf.len() {
        return None;
    }
    if memory.read(caller, wasm_ptr as usize, &mut buf[..len]).is_err() {
        return None;
    }
    std::str::from_utf8(&buf[..len]).ok()
}

pub fn define_host_calls(linker: &mut Linker<StoreState>) -> anyhow::Result<()> {
    // ── Generic multiplexed host call (for non-hot-path operations) ───────────
    linker.func_wrap(
        "interstice",
        "interstice_host_call",
        |mut caller: Caller<'_, StoreState>, ptr: i32, len: i32| -> i64 {
            let memory = match caller.get_export("memory") {
                Some(wasmtime::Extern::Memory(mem)) => mem,
                _ => return 0,
            };
            let data = caller.data();
            let module_schema = data.module_schema.clone();
            let runtime = data.runtime.clone();
            match runtime.dispatch_host_call(&memory, &mut caller, module_schema, ptr, len) {
                Ok(Some(result)) => result,
                Ok(None) => 0,
                Err(err) => {
                    runtime.logger.log(
                        &format!("An error occured when dispatching the host call: {}", err),
                        LogSource::Runtime,
                        LogLevel::Error,
                    );
                    0
                }
            }
        },
    )?;

    // ── Direct: fail ─────────────────────────────────────────────────────────
    // The guest says why the current call fails, just before it traps.
    linker.func_wrap(
        "interstice",
        "interstice_fail",
        |mut caller: Caller<'_, StoreState>, ptr: i32, len: i32| {
            let Some(wasmtime::Extern::Memory(memory)) = caller.get_export("memory") else { return };
            let mut buf = vec![0u8; len.clamp(0, 64 * 1024) as usize];
            if memory.read(&caller, ptr.max(0) as usize, &mut buf).is_ok() {
                caller.data_mut().failure = Some(String::from_utf8_lossy(&buf).into_owned());
            }
        },
    )?;

    // ── Direct: log ───────────────────────────────────────────────────────────
    linker.func_wrap(
        "interstice",
        "interstice_log",
        |mut caller: Caller<'_, StoreState>, ptr: i32, len: i32| {
            let memory = match caller.get_export("memory") {
                Some(wasmtime::Extern::Memory(mem)) => mem,
                _ => return,
            };
            let module_name = caller.data().module_schema.name.clone();
            let runtime = caller.data().runtime.clone();
            let mut buf = vec![0u8; len.max(0) as usize];
            if memory.read(&mut caller, (ptr as u32) as usize, &mut buf).is_ok() {
                let msg = String::from_utf8_lossy(&buf).into_owned();
                runtime.handle_log(module_name, interstice_abi::LogRequest { message: msg });
            }
        },
    )?;

    // ── Direct: time ──────────────────────────────────────────────────────────
    linker.func_wrap(
        "interstice",
        "interstice_time",
        |caller: Caller<'_, StoreState>| -> i64 {
            let runtime = caller.data().runtime.clone();
            match runtime.handle_time(interstice_abi::TimeRequest {}) {
                interstice_abi::TimeResponse::Ok { unix_ms } => unix_ms as i64,
                interstice_abi::TimeResponse::Err(_) => 0,
            }
        },
    )?;

    // ── Direct: random ────────────────────────────────────────────────────────
    linker.func_wrap(
        "interstice",
        "interstice_random",
        |caller: Caller<'_, StoreState>| -> i64 {
            let runtime = caller.data().runtime.clone();
            match runtime.handle_deterministic_random(interstice_abi::DeterministicRandomRequest {}) {
                interstice_abi::DeterministicRandomResponse::Ok(v) => v as i64,
                interstice_abi::DeterministicRandomResponse::Err(_) => 0,
            }
        },
    )?;

    // ── Direct: blobs ─────────────────────────────────────────────────────────
    // Copy `len` bytes of guest memory into a new blob owned by the current
    // call frame. Returns the blob id, or -1.
    linker.func_wrap(
        "interstice",
        "interstice_blob_create",
        |mut caller: Caller<'_, StoreState>, ptr: i32, len: i32| -> i64 {
            let Some(wasmtime::Extern::Memory(memory)) = caller.get_export("memory") else {
                return -1;
            };
            let (start, len) = ((ptr as u32) as usize, (len as u32) as usize);
            let Some(bytes) = memory.data(&caller).get(start..start + len) else {
                return -1;
            };
            match crate::runtime::blob::create_blob(bytes.into()) {
                Some(id) => id as i64,
                None => -1,
            }
        },
    )?;

    // Length of a blob visible to the current call, or -1.
    linker.func_wrap(
        "interstice",
        "interstice_blob_len",
        |_caller: Caller<'_, StoreState>, id: i64| -> i64 {
            crate::runtime::blob::resolve_blob(id as u64).map_or(-1, |b| b.len() as i64)
        },
    )?;

    // Copy up to `len` bytes of the blob, from `offset`, into guest memory.
    // Returns the number of bytes copied, or -1.
    linker.func_wrap(
        "interstice",
        "interstice_blob_read",
        |mut caller: Caller<'_, StoreState>, id: i64, offset: i64, ptr: i32, len: i32| -> i64 {
            let Some(wasmtime::Extern::Memory(memory)) = caller.get_export("memory") else {
                return -1;
            };
            let Some(blob) = crate::runtime::blob::resolve_blob(id as u64) else {
                return -1;
            };
            let offset = (offset.max(0) as usize).min(blob.len());
            let n = ((len as u32) as usize).min(blob.len() - offset);
            match memory.write(&mut caller, (ptr as u32) as usize, &blob[offset..offset + n]) {
                Ok(()) => n as i64,
                Err(_) => -1,
            }
        },
    )?;

    // ── Direct: insert_row ────────────────────────────────────────────────────
    linker.func_wrap(
        "interstice",
        "interstice_insert_row",
        |mut caller: Caller<'_, StoreState>,
         table_ptr: i32,
         table_len: i32,
         row_ptr: i32,
         row_len: i32,
         resp_ptr: i32,
         resp_cap: i32|
         -> i32 {
            let _span = crate::runtime::profile::span(crate::runtime::profile::Phase::HostInsert);
            let memory = match caller.get_export("memory") {
                Some(wasmtime::Extern::Memory(mem)) => mem,
                _ => return -1,
            };
            // Stack buffer for table name (avoids heap alloc for short strings).
            let mut table_name_buf = [0u8; 256];
            let table_name = match read_str_stack(&mut caller, &memory, table_ptr, table_len, &mut table_name_buf) {
                Some(s) => s,
                None => return -1,
            };
            // Zero-copy decode: borrow WASM linear memory directly instead of
            // copying into a Vec first.  The borrow is dropped as soon as `row`
            // is constructed (all fields are owned).
            let row_start = row_ptr.max(0) as usize;
            let row_end = row_start.saturating_add(row_len.max(0) as usize);
            let row: Row = {
                let wasm_data = memory.data(&caller);
                if row_end > wasm_data.len() { return -1; }
                match decode(&wasm_data[row_start..row_end]) {
                    Ok(r) => r,
                    Err(_) => return -1,
                }
            };
            // Retry path: a prior call overflowed the wasm's fast-path buffer and
            // stashed its encoded response.  The SDK's `insert_row` retries with a
            // larger buffer immediately and synchronously — no other host call can
            // run in between — so the very next insert_row IS that retry.  Return
            // the stashed bytes WITHOUT re-running the side-effecting insert, so the
            // transaction is applied exactly once.
            if let Some(pending) = caller.data_mut().pending_insert_response.take() {
                if pending.len() > resp_cap as usize {
                    // Caller still didn't size the buffer correctly — re-stash and
                    // ask again.  (The SDK always allocates the exact size, so this
                    // should not happen.)
                    let needed = pending.len();
                    caller.data_mut().pending_insert_response = Some(pending);
                    return -(needed as i32);
                }
                if memory.write(&mut caller, (resp_ptr as u32) as usize, &pending).is_err() {
                    return -1;
                }
                return pending.len() as i32;
            }
            let module_schema = caller.data().module_schema.clone();
            let runtime = caller.data().runtime.clone();
            let response = runtime.handle_insert_row(&*module_schema, InsertRowRequest { table_name: table_name.to_string(), row });
            // Fast path: Ok(None) is always [0x00, 0x00] in postcard (variant 0,
            // Option::None = 0).  Skip the encode() alloc entirely.
            match response {
                interstice_abi::InsertRowResponse::Ok(None) => {
                    if resp_cap < 2 { return -2; }
                    if memory.write(&mut caller, (resp_ptr as u32) as usize, &[0x00, 0x00]).is_err() {
                        return -1;
                    }
                    2
                }
                _ => {
                    let encoded = match encode(&response) {
                        Ok(b) => b,
                        Err(_) => return -1,
                    };
                    if encoded.len() > resp_cap as usize {
                        // Response doesn't fit.  The insert side effect has already
                        // been applied; stash the encoded response so the SDK's
                        // immediate retry returns it without re-applying.
                        let needed = encoded.len();
                        caller.data_mut().pending_insert_response = Some(encoded);
                        return -(needed as i32);
                    }
                    if memory.write(&mut caller, (resp_ptr as u32) as usize, &encoded).is_err() {
                        return -1;
                    }
                    encoded.len() as i32
                }
            }
        },
    )?;

    // ── Direct: update_row ────────────────────────────────────────────────────
    linker.func_wrap(
        "interstice",
        "interstice_update_row",
        |mut caller: Caller<'_, StoreState>,
         table_ptr: i32,
         table_len: i32,
         row_ptr: i32,
         row_len: i32|
         -> i32 {
            let _span = crate::runtime::profile::span(crate::runtime::profile::Phase::HostUpdate);
            let memory = match caller.get_export("memory") {
                Some(wasmtime::Extern::Memory(mem)) => mem,
                _ => return -1,
            };
            let mut table_name_buf = [0u8; 256];
            let table_name = match read_str_stack(&mut caller, &memory, table_ptr, table_len, &mut table_name_buf) {
                Some(s) => s,
                None => return -1,
            };
            let row_start = row_ptr.max(0) as usize;
            let row_end = row_start.saturating_add(row_len.max(0) as usize);
            let row: Row = {
                let wasm_data = memory.data(&caller);
                if row_end > wasm_data.len() { return -1; }
                match decode(&wasm_data[row_start..row_end]) {
                    Ok(r) => r,
                    Err(_) => return -1,
                }
            };
            let module_schema = caller.data().module_schema.clone();
            let runtime = caller.data().runtime.clone();
            match runtime.handle_update_row(&*module_schema, interstice_abi::UpdateRowRequest { table_name: table_name.to_string(), row }) {
                interstice_abi::UpdateRowResponse::Ok => 0,
                interstice_abi::UpdateRowResponse::Err(_) => -1,
            }
        },
    )?;

    // ── Direct: delete_row ────────────────────────────────────────────────────
    linker.func_wrap(
        "interstice",
        "interstice_delete_row",
        |mut caller: Caller<'_, StoreState>,
         table_ptr: i32,
         table_len: i32,
         pk_ptr: i32,
         pk_len: i32|
         -> i32 {
            let _span = crate::runtime::profile::span(crate::runtime::profile::Phase::HostDelete);
            let memory = match caller.get_export("memory") {
                Some(wasmtime::Extern::Memory(mem)) => mem,
                _ => return -1,
            };
            let mut table_name_buf = [0u8; 256];
            let table_name = match read_str_stack(&mut caller, &memory, table_ptr, table_len, &mut table_name_buf) {
                Some(s) => s,
                None => return -1,
            };
            let pk_start = pk_ptr.max(0) as usize;
            let pk_end = pk_start.saturating_add(pk_len.max(0) as usize);
            let primary_key: IndexKey = {
                let wasm_data = memory.data(&caller);
                if pk_end > wasm_data.len() { return -1; }
                match decode(&wasm_data[pk_start..pk_end]) {
                    Ok(k) => k,
                    Err(_) => return -1,
                }
            };
            let module_name = caller.data().module_schema.name.clone();
            let runtime = caller.data().runtime.clone();
            match runtime.handle_delete_row(module_name, interstice_abi::DeleteRowRequest { table_name: table_name.to_string(), primary_key }) {
                interstice_abi::DeleteRowResponse::Ok => 0,
                interstice_abi::DeleteRowResponse::Err(_) => -1,
            }
        },
    )?;

    // ── Direct: clear_table (current module) ──────────────────────────────────
    linker.func_wrap(
        "interstice",
        "interstice_clear_table",
        |mut caller: Caller<'_, StoreState>, table_ptr: i32, table_len: i32| -> i32 {
            let _span = crate::runtime::profile::span(crate::runtime::profile::Phase::HostOther);
            let memory = match caller.get_export("memory") {
                Some(wasmtime::Extern::Memory(mem)) => mem,
                _ => return -1,
            };
            let mut table_name_buf = [0u8; 256];
            let table_name = match read_str_stack(&mut caller, &memory, table_ptr, table_len, &mut table_name_buf) {
                Some(s) => s,
                None => return -1,
            };
            let module_name = caller.data().module_schema.name.clone();
            let runtime = caller.data().runtime.clone();
            match runtime.handle_clear_table(
                module_name,
                interstice_abi::ClearTableRequest {
                    module_selection: interstice_abi::ModuleSelection::Current,
                    table_name: table_name.to_string(),
                },
            ) {
                interstice_abi::ClearTableResponse::Ok => 0,
                interstice_abi::ClearTableResponse::Err(_) => -1,
            }
        },
    )?;

    // ── Direct: get_by_pk ────────────────────────────────────────────────────
    linker.func_wrap(
        "interstice",
        "interstice_get_by_pk",
        |mut caller: Caller<'_, StoreState>,
         table_ptr: i32,
         table_len: i32,
         pk_ptr: i32,
         pk_len: i32,
         resp_ptr: i32,
         resp_cap: i32|
         -> i32 {
            let _span = crate::runtime::profile::span(crate::runtime::profile::Phase::HostGet);
            let memory = match caller.get_export("memory") {
                Some(wasmtime::Extern::Memory(mem)) => mem,
                _ => return -1,
            };
            let mut table_name_buf = [0u8; 256];
            let table_name = match read_str_stack(&mut caller, &memory, table_ptr, table_len, &mut table_name_buf) {
                Some(s) => s,
                None => return -1,
            };
            let pk_start = pk_ptr.max(0) as usize;
            let pk_end = pk_start.saturating_add(pk_len.max(0) as usize);
            let primary_key: IndexKey = {
                let wasm_data = memory.data(&caller);
                if pk_end > wasm_data.len() { return -1; }
                match decode(&wasm_data[pk_start..pk_end]) {
                    Ok(k) => k,
                    Err(_) => return -1,
                }
            };
            let runtime = caller.data().runtime.clone();
            let response = runtime.handle_table_get_by_primary_key(TableGetByPrimaryKeyRequest {
                module_selection: ModuleSelection::Current,
                table_name: table_name.to_string(),
                primary_key,
            });
            let encoded = match encode(&response) {
                Ok(b) => b,
                Err(_) => return -1,
            };
            if encoded.len() > resp_cap as usize {
                return -(encoded.len() as i32);
            }
            if memory.write(&mut caller, (resp_ptr as u32) as usize, &encoded).is_err() {
                return -1;
            }
            encoded.len() as i32
        },
    )?;

    Ok(())
}
