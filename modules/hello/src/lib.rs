use interstice_abi::codec::unpack_ptr_len;
use interstice_abi::schema::{EntrySchema, TableSchema, TableVisibility};
use interstice_abi::{
    codec::pack_ptr_len, decode, encode, HostCall, LogRequest, ModuleSchema, PrimitiveType,
    PrimitiveValue, ReducerSchema,
};
use interstice_abi::{InsertRowRequest, Row, TableScanRequest};
use std::alloc::{alloc as local_alloc, dealloc as local_dealloc, Layout};
use std::slice;

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

// Requirement for interstice module

#[no_mangle]
pub extern "C" fn alloc(size: i32) -> i32 {
    let layout = Layout::from_size_align(size as usize, 8).unwrap();
    unsafe { local_alloc(layout) as i32 }
}

#[no_mangle]
pub extern "C" fn dealloc(ptr: i32, size: i32) {
    let layout = Layout::from_size_align(size as usize, 8).unwrap();
    unsafe { local_dealloc(ptr as *mut u8, layout) }
}

#[no_mangle]
pub extern "C" fn interstice_describe() -> i64 {
    let schema = ModuleSchema::new(
        "hello",
        1,
        vec![ReducerSchema::new(
            "hello",
            vec![EntrySchema {
                name: "name".to_string(),
                value_type: PrimitiveType::String,
            }],
            PrimitiveType::String,
        )],
        vec![TableSchema {
            name: "greetings".to_string(),
            visibility: TableVisibility::Public,
            entries: vec![EntrySchema {
                name: "greeting".to_string(),
                value_type: PrimitiveType::String,
            }],
            primary_key: EntrySchema {
                name: "id".to_string(),
                value_type: PrimitiveType::I64,
            },
        }],
    );

    let bytes = encode(&schema).unwrap();

    let ptr = alloc(bytes.len() as i32);
    unsafe {
        core::slice::from_raw_parts_mut(ptr as *mut u8, bytes.len()).copy_from_slice(&bytes);
    }

    pack_ptr_len(ptr, bytes.len() as i32)
}

// raw ABI import
#[link(wasm_import_module = "interstice")]
extern "C" {
    fn interstice_host_call(ptr: i32, len: i32) -> i64;
}

fn host_log(message: &str) {
    let call = HostCall::Log(LogRequest {
        message: message.to_string(),
    });

    let bytes = encode(&call).unwrap();

    unsafe {
        interstice_host_call(bytes.as_ptr() as i32, bytes.len() as i32);
    }
}

fn host_insert_row(id: i64, greeting: String) {
    let call = HostCall::InsertRow(InsertRowRequest {
        table: "greetings".to_string(),
        row: Row {
            primary_key: PrimitiveValue::I64(id),
            entries: vec![PrimitiveValue::String(greeting)],
        },
    });

    let bytes = encode(&call).unwrap();

    unsafe {
        interstice_host_call(bytes.as_ptr() as i32, bytes.len() as i32);
    }
}

fn host_scan() -> Vec<Row> {
    let call = HostCall::TableScan(TableScanRequest {
        table: "greetings".to_string(),
    });

    let bytes = encode(&call).unwrap();

    let pack = unsafe { interstice_host_call(bytes.as_ptr() as i32, bytes.len() as i32) };
    let (ptr, len) = unpack_ptr_len(pack);
    let bytes = unsafe { slice::from_raw_parts(ptr as *const u8, len as usize) };
    let rows: Vec<Row> = decode(bytes).unwrap();
    return rows;
}

// REDUCERS

#[no_mangle]
pub extern "C" fn hello(ptr: i32, len: i32) -> i64 {
    let bytes = unsafe { slice::from_raw_parts(ptr as *const u8, len as usize) };
    let input: PrimitiveValue = decode(bytes).unwrap();

    host_log("Previous greetings: ");
    host_scan().iter().for_each(|row| {
        if let PrimitiveValue::String(greeting) = &row.entries[0] {
            host_log(&format!("- {}", greeting));
        }
    });

    let mut msg = PrimitiveValue::String("Default Hello".to_string());
    if let PrimitiveValue::String(name) = input {
        msg = PrimitiveValue::String(format!("Hello, {}!", name));

        host_log("Inserting greeting...");
        host_insert_row(1, format!("Hello, {}!", name));
    }

    let bytes = encode(&msg).unwrap();
    let out_ptr = alloc(bytes.len() as i32);
    unsafe {
        slice::from_raw_parts_mut(out_ptr as *mut u8, bytes.len()).copy_from_slice(&bytes);
    }
    return pack_ptr_len(out_ptr, bytes.len() as i32);
}
