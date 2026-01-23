use std::sync::{Arc, Mutex};

use crate::interstice_host_call;
use interstice_abi::{
    HostCall, InsertRowRequest, LogRequest, ModuleSchema, ReducerSchema, Row, TableScanRequest,
    codec::{pack_ptr_len, unpack_ptr_len},
    decode, encode,
    schema::Version,
};

pub fn describe_module(name: &str, version: &str) -> i64 {
    let reducers = crate::registry::collect_reducers();
    let tables = crate::registry::collect_tables();
    let subscriptions = crate::registry::collect_subscriptions();

    let schema = ModuleSchema {
        abi_version: interstice_abi::ABI_VERSION,
        name: name.to_string(),
        version: parse_version(version),
        reducers,
        tables,
        subscriptions,
    };

    let bytes = encode(&schema).unwrap();
    let len = bytes.len() as i32;
    let ptr = Box::into_raw(bytes.into_boxed_slice()) as *mut u8 as i32;
    return pack_ptr_len(ptr, len);
}

fn parse_version(version: &str) -> Version {
    let parts: Vec<&str> = version.split('.').collect();
    let major = parts.get(0).and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    Version {
        major,
        minor,
        patch,
    }
}

pub struct Context;

impl Context {
    pub fn log(&self, message: &str) {
        let call = HostCall::Log(LogRequest {
            message: message.to_string(),
        });

        let bytes = encode(&call).unwrap();

        unsafe {
            interstice_host_call(bytes.as_ptr() as i32, bytes.len() as i32);
        }
    }
    pub fn insert_row(&self, table_name: String, row: Row) {
        let call = HostCall::InsertRow(InsertRowRequest { table_name, row });

        let bytes = encode(&call).unwrap();

        unsafe {
            interstice_host_call(bytes.as_ptr() as i32, bytes.len() as i32);
        }
    }
    pub fn scan(&self, table_name: String) -> Vec<Row> {
        let call = HostCall::TableScan(TableScanRequest { table_name });

        let bytes = encode(&call).unwrap();

        let pack = unsafe { interstice_host_call(bytes.as_ptr() as i32, bytes.len() as i32) };
        let (ptr, len) = unpack_ptr_len(pack);
        let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
        let rows: Vec<Row> = decode(bytes).unwrap();
        return rows;
    }
}
