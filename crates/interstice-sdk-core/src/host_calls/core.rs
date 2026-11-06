use interstice_abi::{
    CallQueryRequest, CallQueryResponse, CallReducerRequest, CallReducerResponse, HostCall,
    IndexKey, IndexQuery, IntersticeValue, ModuleSelection, NodeSelection, ScheduleRequest,
    ScheduleResponse, TableGetByPrimaryKeyRequest, TableIndexScanRequest, TableScanRequest, Row,
    encode, wire,
};

use crate::table_row::RowCodec;

// Pre-allocated scratch buffer for serialising rows/keys before a direct host call.
// WASM is single-threaded so a module-level static is safe to use as a reusable scratch area.
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

pub type NodeId = String;

/// Tell the host why the current call fails; it reports this
/// message to the caller instead of the trap that follows.
pub fn fail(message: &str) {
    let msg = message.as_bytes();
    unsafe { crate::host_calls::interstice_fail(msg.as_ptr() as i32, msg.len() as i32) };
}

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

/// Run the zero-argument reducer `reducer_name` of this module `delay_ms`
/// after the current transaction commits. Nothing is scheduled if it fails.
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

/// The next value of the transaction's random stream: reproducible from the
/// transaction's seed, shared with the queries it calls.
pub fn deterministic_random_u64() -> Result<u64, String> {
    let v = unsafe { crate::host_calls::interstice_random() };
    Ok(v as u64)
}

/// The transaction's timestamp (unix ms): the same value for every call in
/// one transaction, and never lower than an earlier transaction's on this
/// node. Time does not advance while a reducer runs.
pub fn time_now_ms() -> Result<u64, String> {
    let v = unsafe { crate::host_calls::interstice_time() };
    Ok(v as u64)
}

/// A reusable buffer for the encoded row of a direct host call (WASM is
/// single-threaded, and no host call re-enters the code that holds it).
static mut ROW_BUF: Vec<u8> = Vec::new();

fn with_row_buf<R>(f: impl FnOnce(&mut Vec<u8>) -> R) -> R {
    // SAFETY: single-threaded, and not re-entered (see `ROW_BUF`).
    let buf = unsafe { &mut *std::ptr::addr_of_mut!(ROW_BUF) };
    buf.clear();
    f(buf)
}

/// A plain postcard string (no value tag).
fn take_string(input: &mut &[u8]) -> Result<String, String> {
    let len = wire::take_len(input).map_err(|e| e.to_string())?;
    let bytes = wire::take_bytes(input, len).map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(bytes).into_owned())
}

/// `Ok(Option<Row>) | Err(String)` (insert and get responses), with the
/// row decoded as `T`.
fn decode_option_row<T: RowCodec>(mut input: &[u8]) -> Result<Option<T>, String> {
    let input = &mut input;
    match wire::take_varint(input).map_err(|e| e.to_string())? {
        0 => match wire::take_byte(input).map_err(|e| e.to_string())? {
            0 => Ok(None),
            _ => T::decode_row(input).map(Some).map_err(|e| e.to_string()),
        },
        _ => Err(take_string(input)?),
    }
}

/// `Ok { rows: Vec<Row> } | Err(String)` (scan responses), with the rows
/// decoded as `T`.
fn decode_rows<T: RowCodec>(mut input: &[u8]) -> Result<Vec<T>, String> {
    let input = &mut input;
    match wire::take_varint(input).map_err(|e| e.to_string())? {
        0 => {
            let len = wire::take_len(input).map_err(|e| e.to_string())?;
            let mut rows = Vec::with_capacity(len.min(input.len()));
            for _ in 0..len {
                rows.push(T::decode_row(input).map_err(|e| e.to_string())?);
            }
            Ok(rows)
        }
        _ => Err(take_string(input)?),
    }
}

fn direct_resp() -> (i32, i32) {
    let ptr = std::ptr::addr_of_mut!(crate::host_calls::DIRECT_RESP_BUF) as *mut u8 as i32;
    (ptr, crate::host_calls::DIRECT_RESP_BUF_CAP as i32)
}

fn direct_resp_bytes(written: i32) -> &'static [u8] {
    // SAFETY: the host wrote `written` bytes there; read before the next call.
    unsafe {
        std::slice::from_raw_parts(std::ptr::addr_of!(crate::host_calls::DIRECT_RESP_BUF) as *const u8, written as usize)
    }
}

/// Insert `row`; returns it, with its auto-increment key if it has one.
pub fn insert_row<T: RowCodec>(table_name: &str, row: T) -> Result<T, String> {
    let table = table_name.as_bytes();
    let modified = with_row_buf(|buf| {
        row.encode_row(buf);
        let call = |out_ptr: i32, out_cap: i32| unsafe {
            crate::host_calls::interstice_insert_row(
                table.as_ptr() as i32,
                table.len() as i32,
                buf.as_ptr() as i32,
                buf.len() as i32,
                out_ptr,
                out_cap,
            )
        };
        let (ptr, cap) = direct_resp();
        let written = call(ptr, cap);
        if written >= 0 {
            return decode_option_row::<T>(direct_resp_bytes(written));
        }
        // The response is larger than the direct buffer: the host returned
        // -(needed size).
        let mut heap = vec![0u8; (-written) as usize];
        let written = call(heap.as_mut_ptr() as i32, heap.len() as i32);
        if written < 0 {
            return Err("insert_row: response buffer too small".into());
        }
        decode_option_row::<T>(&heap[..written as usize])
    })?;
    Ok(modified.unwrap_or(row))
}

pub fn update_row<T: RowCodec>(table_name: &str, row: &T) -> Result<(), String> {
    let table = table_name.as_bytes();
    let result = with_row_buf(|buf| {
        row.encode_row(buf);
        unsafe {
            crate::host_calls::interstice_update_row(
                table.as_ptr() as i32,
                table.len() as i32,
                buf.as_ptr() as i32,
                buf.len() as i32,
            )
        }
    });
    if result < 0 { Err("update_row failed".into()) } else { Ok(()) }
}

pub fn delete_row(table_name: &str, primary_key: IndexKey) -> Result<(), String> {
    let table_bytes = table_name.as_bytes();
    let pk_buf = encode_scratch(&primary_key)?;
    let (pk_ptr, pk_len) = pk_buf.ptr_len();
    let result = unsafe {
        crate::host_calls::interstice_delete_row(
            table_bytes.as_ptr() as i32,
            table_bytes.len() as i32,
            pk_ptr,
            pk_len,
        )
    };
    if result < 0 {
        Err("delete_row failed".into())
    } else {
        Ok(())
    }
}

pub fn clear_table(module_selection: ModuleSelection, table_name: &str) -> Result<(), String> {
    match module_selection {
        ModuleSelection::Current => {
            let table_bytes = table_name.as_bytes();
            let result = unsafe {
                crate::host_calls::interstice_clear_table(
                    table_bytes.as_ptr() as i32,
                    table_bytes.len() as i32,
                )
            };
            if result < 0 {
                Err("clear_table failed".into())
            } else {
                Ok(())
            }
        }
        ModuleSelection::Other(_) => {
            Err("clear_table: only the current module's tables can be cleared".into())
        }
    }
}

pub fn scan<T: RowCodec>(module_selection: ModuleSelection, table_name: String) -> Result<Vec<T>, String> {
    let call = HostCall::TableScan(TableScanRequest {
        module_selection,
        table_name,
    });
    decode_rows(unpack_bytes(host_call(call)))
}

pub fn get_by_primary_key<T: RowCodec>(
    module_selection: ModuleSelection,
    table_name: &str,
    primary_key: IndexKey,
) -> Result<Option<T>, String> {
    // Fast path for the current module: the response goes straight into the
    // fixed direct buffer. A row too large for it takes the general,
    // dynamically sized path below.
    if let ModuleSelection::Current = module_selection {
        let table_bytes = table_name.as_bytes();
        let pk_buf = encode_scratch(&primary_key)?;
        let (pk_ptr, pk_len) = pk_buf.ptr_len();
        let (resp_ptr, resp_cap) = direct_resp();
        let written = unsafe {
            crate::host_calls::interstice_get_by_pk(
                table_bytes.as_ptr() as i32,
                table_bytes.len() as i32,
                pk_ptr,
                pk_len,
                resp_ptr,
                resp_cap,
            )
        };
        if written >= 0 {
            return decode_option_row(direct_resp_bytes(written));
        }
    }

    let call = HostCall::TableGetByPrimaryKey(TableGetByPrimaryKeyRequest {
        module_selection,
        table_name: table_name.to_string(),
        primary_key,
    });
    decode_option_row(unpack_bytes(host_call(call)))
}

pub fn scan_index<T: RowCodec>(
    module_selection: ModuleSelection,
    table_name: String,
    field_name: String,
    query: IndexQuery,
) -> Result<Vec<T>, String> {
    let call = HostCall::TableIndexScan(TableIndexScanRequest {
        module_selection,
        table_name,
        field_name,
        query,
    });
    decode_rows(unpack_bytes(host_call(call)))
}

use crate::{QueryContext, ReducerContext};

use crate::host_calls::{host_call, unpack, unpack_bytes};

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

impl<Caps> HostLog for ReducerContext<Caps> {
    fn log(&self, message: &str) {
        log(message);
    }
}

impl<Caps> HostLog for QueryContext<Caps> {
    fn log(&self, message: &str) {
        log(message);
    }
}

impl<Caps> HostCurrentNodeId for ReducerContext<Caps> {
    fn current_node_id(&self) -> NodeId {
        current_node_id()
    }
}

impl<Caps> HostCurrentNodeId for QueryContext<Caps> {
    fn current_node_id(&self) -> NodeId {
        current_node_id()
    }
}

impl<Caps> HostTime for ReducerContext<Caps> {
    fn time_now_ms(&self) -> Result<u64, String> {
        time_now_ms()
    }
}

impl<Caps> HostTime for QueryContext<Caps> {
    fn time_now_ms(&self) -> Result<u64, String> {
        time_now_ms()
    }
}

impl<Caps> HostDeterministicRandom for ReducerContext<Caps> {
    fn deterministic_random_u64(&self) -> Result<u64, String> {
        deterministic_random_u64()
    }
}

impl<Caps> HostDeterministicRandom for QueryContext<Caps> {
    fn deterministic_random_u64(&self) -> Result<u64, String> {
        deterministic_random_u64()
    }
}

impl<Caps> HostSchedule for ReducerContext<Caps> {
    fn schedule(&self, reducer_name: &str, delay_ms: u64) -> Result<(), String> {
        schedule(reducer_name.to_string(), delay_ms)
    }
}

// ── History of logged tables (DESIGN §6.7) ───────────────────────────────────

fn history_call(call: interstice_abi::HistoryCall) -> interstice_abi::HistoryResponse {
    unpack(host_call(HostCall::History(call)))
}

/// The newest `limit` commits of a logged table of the current module,
/// newest first.
pub fn history_commits(
    table_name: &str,
    limit: u32,
) -> Result<Vec<interstice_abi::HistoryCommit>, String> {
    use interstice_abi::{HistoryCall, HistoryResponse};
    match history_call(HistoryCall::Commits { table_name: table_name.into(), limit }) {
        HistoryResponse::Commits(commits) => Ok(commits),
        HistoryResponse::Err(err) => Err(err),
        other => Err(format!("unexpected history response {other:?}")),
    }
}

/// The rows of a logged table at `point`, by primary key.
pub fn history_rows_at(
    table_name: &str,
    point: interstice_abi::HistoryPoint,
) -> Result<Vec<Row>, String> {
    use interstice_abi::{HistoryCall, HistoryResponse};
    match history_call(HistoryCall::RowsAt { table_name: table_name.into(), point }) {
        HistoryResponse::Rows(rows) => Ok(rows),
        HistoryResponse::Err(err) => Err(err),
        other => Err(format!("unexpected history response {other:?}")),
    }
}

/// Stage a restore of a logged table to `point`; returns the `tx` of the
/// last log entry the point covers (`0` for an empty start).
pub fn history_restore(table_name: &str, point: interstice_abi::HistoryPoint) -> Result<u64, String> {
    use interstice_abi::{HistoryCall, HistoryResponse};
    match history_call(HistoryCall::Restore { table_name: table_name.into(), point }) {
        HistoryResponse::Restored(to) => Ok(to),
        HistoryResponse::Err(err) => Err(err),
        other => Err(format!("unexpected history response {other:?}")),
    }
}
