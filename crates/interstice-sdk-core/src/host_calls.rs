use interstice_abi::{
    CallReducerRequest, HostCall, InsertRowRequest, IntersticeValue, LogRequest, ModuleSelection,
    Row, TableScanRequest, decode, encode, unpack_ptr_len,
};

#[link(wasm_import_module = "interstice")]
unsafe extern "C" {
    pub fn interstice_host_call(ptr: i32, len: i32) -> i64;
}

pub fn log(message: &str) {
    let call = HostCall::Log(LogRequest {
        message: message.to_string(),
    });

    let bytes = encode(&call).unwrap();

    unsafe {
        interstice_host_call(bytes.as_ptr() as i32, bytes.len() as i32);
    }
}

pub fn call_reducer(
    module_selection: ModuleSelection,
    reducer_name: String,
    input: IntersticeValue,
) -> IntersticeValue {
    let call = HostCall::CallReducer(CallReducerRequest {
        module_selection,
        reducer_name,
        input,
    });

    let bytes = encode(&call).unwrap();

    let pack = unsafe { interstice_host_call(bytes.as_ptr() as i32, bytes.len() as i32) };
    let (ptr, len) = unpack_ptr_len(pack);
    let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
    let result: IntersticeValue = decode(bytes).unwrap();
    return result;
}

pub fn insert_row(module_selection: ModuleSelection, table_name: String, row: Row) {
    let call = HostCall::InsertRow(InsertRowRequest {
        module_selection,
        table_name,
        row,
    });

    let bytes = encode(&call).unwrap();

    unsafe {
        interstice_host_call(bytes.as_ptr() as i32, bytes.len() as i32);
    }
}
pub fn scan(module_selection: ModuleSelection, table_name: String) -> Vec<Row> {
    let call = HostCall::TableScan(TableScanRequest {
        module_selection,
        table_name,
    });

    let bytes = encode(&call).unwrap();

    let pack = unsafe { interstice_host_call(bytes.as_ptr() as i32, bytes.len() as i32) };
    let (ptr, len) = unpack_ptr_len(pack);
    let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
    let rows: Vec<Row> = decode(bytes).unwrap();
    return rows;
}
