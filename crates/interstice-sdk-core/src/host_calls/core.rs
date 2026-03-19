use interstice_abi::{
    CallQueryRequest, CallQueryResponse, CallReducerRequest, CallReducerResponse,
    HostCall, IndexKey, IndexQuery,
    InsertRowResponse, IntersticeValue, ModuleSelection,
    NodeSelection, Row, ScheduleRequest, ScheduleResponse, TableGetByPrimaryKeyRequest,
    TableGetByPrimaryKeyResponse, TableIndexScanRequest, TableIndexScanResponse, TableScanRequest,
    TableScanResponse,
    decode, encode,
};

// Pre-allocated scratch buffer for serialising rows/keys before a direct host call.
// WASM is single-threaded so a module-level static is safe to use as a reusable scratch area.
#[cfg(target_arch = "wasm32")]
static mut ENCODE_BUF: [u8; 16384] = [0u8; 16384];

/// Holds encoded bytes for a direct host call — either borrowed from the static scratch buffer
/// (zero alloc, common path) or owned on the heap (fallback for unusually large values).
enum EncodedBuf<'a> {
    Static(&'a [u8]),
    Heap(Vec<u8>),
}

impl EncodedBuf<'_> {
    #[inline]
    fn ptr_len(&self) -> (i32, i32) {
        match self {
            Self::Static(s) => (s.as_ptr() as i32, s.len() as i32),
            Self::Heap(v) => (v.as_ptr() as i32, v.len() as i32),
        }
    }
}

/// Serialize `value` into the static encode buffer when possible, falling back to a heap
/// allocation for values that exceed the static buffer.
#[cfg(target_arch = "wasm32")]
#[inline]
fn encode_scratch<T: serde::Serialize>(value: &T) -> Result<EncodedBuf<'static>, String> {
    unsafe {
        // Rust 2024: use addr_of_mut! to avoid creating a &mut reference to a static.
        let buf_ptr = std::ptr::addr_of_mut!(ENCODE_BUF);
        let scratch = std::slice::from_raw_parts_mut(buf_ptr as *mut u8, 16384);
        match postcard::to_slice(value, scratch) {
            Ok(slice) => {
                // SAFETY: the static reference is valid for the duration of the call (WASM is
                // single-threaded; nothing else can observe the buffer between here and the
                // corresponding host call).
                let static_slice: &'static [u8] = &*(slice as *const [u8]);
                Ok(EncodedBuf::Static(static_slice))
            }
            Err(_) => {
                let v = encode(value).map_err(|e| e.to_string())?;
                Ok(EncodedBuf::Heap(v))
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn encode_scratch<T: serde::Serialize>(value: &T) -> Result<EncodedBuf<'static>, String> {
    let v = encode(value).map_err(|e| e.to_string())?;
    Ok(EncodedBuf::Heap(v))
}

pub type NodeId = String;

pub fn log(message: &str) {
    let msg = message.as_bytes();
    unsafe { crate::host_calls::interstice_log(msg.as_ptr() as i32, msg.len() as i32) };
}

pub fn current_node_id() -> NodeId {
    let call = HostCall::CurrentNodeId;
    let pack = host_call(call);
    let response: NodeId = unpack(pack);
    return response;
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

pub fn schedule(reducer_name: String, delay_ms: u64) -> Result<(), String> {
    let call = HostCall::Schedule(ScheduleRequest {
        reducer_name,
        delay_ms,
    });

    let pack = host_call(call);
    let response: ScheduleResponse = unpack(pack);
    match response {
        ScheduleResponse::Ok => Ok(()),
        ScheduleResponse::Err(err) => Err(err),
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
    let v = unsafe { crate::host_calls::interstice_random() };
    Ok(v as u64)
}

pub fn time_now_ms() -> Result<u64, String> {
    let v = unsafe { crate::host_calls::interstice_time() };
    Ok(v as u64)
}

pub fn insert_row(table_name: &str, row: Row) -> Result<Row, String> {
    let table_bytes = table_name.as_bytes();
    let row_buf = encode_scratch(&row)?;
    let (row_ptr, row_len) = row_buf.ptr_len();
    #[cfg(target_arch = "wasm32")]
    {
        // Fast path: try the static response buffer first.
        let written = unsafe {
            let resp_ptr = std::ptr::addr_of_mut!(crate::host_calls::DIRECT_RESP_BUF) as *mut u8 as i32;
            crate::host_calls::interstice_insert_row(
                table_bytes.as_ptr() as i32, table_bytes.len() as i32,
                row_ptr, row_len,
                resp_ptr, 8192i32,
            )
        };
        if written >= 0 {
            let resp: InsertRowResponse = unsafe {
                let buf = std::slice::from_raw_parts(
                    std::ptr::addr_of!(crate::host_calls::DIRECT_RESP_BUF) as *const u8,
                    written as usize,
                );
                decode(buf).map_err(|e| e.to_string())?
            };
            return match resp {
                InsertRowResponse::Ok(None) => Ok(row),
                InsertRowResponse::Ok(Some(modified)) => Ok(modified),
                InsertRowResponse::Err(err) => Err(err),
            };
        }
        // Slow path: host returned -(needed_size). Allocate a heap buffer and retry.
        let needed = (-written) as usize;
        let mut heap_buf: Vec<u8> = vec![0u8; needed];
        let written2 = unsafe {
            crate::host_calls::interstice_insert_row(
                table_bytes.as_ptr() as i32, table_bytes.len() as i32,
                row_ptr, row_len,
                heap_buf.as_mut_ptr() as i32, needed as i32,
            )
        };
        if written2 < 0 {
            return Err("insert_row: response buffer too small".into());
        }
        let resp: InsertRowResponse = decode(&heap_buf[..written2 as usize]).map_err(|e| e.to_string())?;
        return match resp {
            InsertRowResponse::Ok(None) => Ok(row),
            InsertRowResponse::Ok(Some(modified)) => Ok(modified),
            InsertRowResponse::Err(err) => Err(err),
        };
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (row_ptr, row_len);
        Err("wasm32 only".into())
    }
}

pub fn update_row(table_name: &str, row: Row) -> Result<(), String> {
    let table_bytes = table_name.as_bytes();
    let row_buf = encode_scratch(&row)?;
    let (row_ptr, row_len) = row_buf.ptr_len();
    let result = unsafe {
        crate::host_calls::interstice_update_row(
            table_bytes.as_ptr() as i32, table_bytes.len() as i32,
            row_ptr, row_len,
        )
    };
    if result < 0 { Err("update_row failed".into()) } else { Ok(()) }
}

pub fn delete_row(table_name: &str, primary_key: IndexKey) -> Result<(), String> {
    let table_bytes = table_name.as_bytes();
    let pk_buf = encode_scratch(&primary_key)?;
    let (pk_ptr, pk_len) = pk_buf.ptr_len();
    let result = unsafe {
        crate::host_calls::interstice_delete_row(
            table_bytes.as_ptr() as i32, table_bytes.len() as i32,
            pk_ptr, pk_len,
        )
    };
    if result < 0 { Err("delete_row failed".into()) } else { Ok(()) }
}

pub fn clear_table(module_selection: ModuleSelection, table_name: &str) -> Result<(), String> {
    match module_selection {
        ModuleSelection::Current => {
            let table_bytes = table_name.as_bytes();
            let result = unsafe {
                crate::host_calls::interstice_clear_table(
                    table_bytes.as_ptr() as i32, table_bytes.len() as i32,
                )
            };
            if result < 0 { Err("clear_table failed".into()) } else { Ok(()) }
        }
        ModuleSelection::Other(_) => {
            Err("clear_table with ModuleSelection::Other is not supported in ABI v2".into())
        }
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
    table_name: &str,
    primary_key: IndexKey,
) -> Result<Option<Row>, String> {
    match module_selection {
        ModuleSelection::Current => {
            let table_bytes = table_name.as_bytes();
            let pk_buf = encode_scratch(&primary_key)?;
            let (pk_ptr, pk_len) = pk_buf.ptr_len();
            let written = unsafe {
                #[cfg(target_arch = "wasm32")]
                let resp_ptr = std::ptr::addr_of_mut!(crate::host_calls::DIRECT_RESP_BUF) as *mut u8 as i32;
                #[cfg(target_arch = "wasm32")]
                let resp_cap = 8192i32;
                #[cfg(not(target_arch = "wasm32"))]
                let resp_ptr = 0i32;
                #[cfg(not(target_arch = "wasm32"))]
                let resp_cap = 0i32;
                crate::host_calls::interstice_get_by_pk(
                    table_bytes.as_ptr() as i32, table_bytes.len() as i32,
                    pk_ptr, pk_len,
                    resp_ptr, resp_cap,
                )
            };
            if written < 0 {
                return Err("get_by_pk: response buffer too small".into());
            }
            #[cfg(target_arch = "wasm32")]
            let resp: TableGetByPrimaryKeyResponse = unsafe {
                let buf = std::slice::from_raw_parts(
                    std::ptr::addr_of!(crate::host_calls::DIRECT_RESP_BUF) as *const u8,
                    written as usize,
                );
                decode(buf).map_err(|e| e.to_string())?
            };
            #[cfg(not(target_arch = "wasm32"))]
            let resp: TableGetByPrimaryKeyResponse = { let _ = written; TableGetByPrimaryKeyResponse::Err("wasm32 only".into()) };
            match resp {
                TableGetByPrimaryKeyResponse::Ok(row) => Ok(row),
                TableGetByPrimaryKeyResponse::Err(err) => Err(err),
            }
        }
        other => {
            let call = HostCall::TableGetByPrimaryKey(TableGetByPrimaryKeyRequest {
                module_selection: other,
                table_name: table_name.to_string(),
                primary_key,
            });
            let pack = host_call(call);
            let response: TableGetByPrimaryKeyResponse = unpack(pack);
            match response {
                TableGetByPrimaryKeyResponse::Ok(row) => Ok(row),
                TableGetByPrimaryKeyResponse::Err(err) => Err(err),
            }
        }
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

pub trait HostCurrentNodeId {
    fn current_node_id(&self) -> NodeId;
}

pub trait HostTime {
    fn time_now_ms(&self) -> Result<u64, String>;
}

pub trait HostDeterministicRandom {
    fn deterministic_random_u64(&self) -> Result<u64, String>;
}

pub trait HostSchedule {
    fn schedule(&self, reducer_name: &str, delay_ms: u64) -> Result<(), String>;
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

impl HostCurrentNodeId for ReducerContext {
    fn current_node_id(&self) -> NodeId {
        current_node_id()
    }
}

impl HostCurrentNodeId for QueryContext {
    fn current_node_id(&self) -> NodeId {
        current_node_id()
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

impl HostSchedule for ReducerContext {
    fn schedule(&self, reducer_name: &str, delay_ms: u64) -> Result<(), String> {
        schedule(reducer_name.to_string(), delay_ms)
    }
}
