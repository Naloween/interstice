use interstice_abi::{
    HostCall, InsertRowRequest, LogRequest, Row, TableScanRequest, decode, encode, unpack_ptr_len,
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
pub fn insert_row(table_name: String, row: Row) {
    let call = HostCall::InsertRow(InsertRowRequest { table_name, row });

    let bytes = encode(&call).unwrap();

    unsafe {
        interstice_host_call(bytes.as_ptr() as i32, bytes.len() as i32);
    }
}
pub fn scan(table_name: String) -> Vec<Row> {
    let call = HostCall::TableScan(TableScanRequest { table_name });

    let bytes = encode(&call).unwrap();

    let pack = unsafe { interstice_host_call(bytes.as_ptr() as i32, bytes.len() as i32) };
    let (ptr, len) = unpack_ptr_len(pack);
    let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
    let rows: Vec<Row> = decode(bytes).unwrap();
    return rows;
}
