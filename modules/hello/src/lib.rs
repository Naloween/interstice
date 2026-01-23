use interstice_abi::IntersticeValue;
use interstice_sdk::{interstice_module, reducer};
use std::alloc::{alloc as local_alloc, dealloc as local_dealloc, Layout};

interstice_module!();

// #[no_mangle]
// pub extern "C" fn interstice_describe() -> i64 {
//     let schema = ModuleSchema::new(
//         "hello",
//         1,
//         vec![
//             ReducerSchema::new(
//                 "hello",
//                 vec![EntrySchema {
//                     name: "name".to_string(),
//                     value_type: PrimitiveType::String,
//                 }],
//                 PrimitiveType::String,
//             ),
//             ReducerSchema::new(
//                 "on_greeting",
//                 vec![EntrySchema {
//                     name: "greeting".to_string(),
//                     value_type: PrimitiveType::String,
//                 }],
//                 PrimitiveType::Void,
//             ),
//         ],
//         vec![TableSchema {
//             name: "greetings".to_string(),
//             visibility: TableVisibility::Public,
//             entries: vec![EntrySchema {
//                 name: "greeting".to_string(),
//                 value_type: PrimitiveType::String,
//             }],
//             primary_key: EntrySchema {
//                 name: "id".to_string(),
//                 value_type: PrimitiveType::I64,
//             },
//         }],
//         vec![SubscriptionSchema {
//             module_name: "hello".to_string(),
//             table_name: "greetings".to_string(),
//             reducer_name: "on_greeting".to_string(),
//             event: TableEvent::Insert,
//         }],
//     );

//     let bytes = encode(&schema).unwrap();

//     let ptr = alloc(bytes.len() as i32);
//     unsafe {
//         core::slice::from_raw_parts_mut(ptr as *mut u8, bytes.len()).copy_from_slice(&bytes);
//     }

//     pack_ptr_len(ptr, bytes.len() as i32)
// }

// fn host_log(message: &str) {
//     let call = HostCall::Log(LogRequest {
//         message: message.to_string(),
//     });

//     let bytes = encode(&call).unwrap();

//     unsafe {
//         interstice_host_call(bytes.as_ptr() as i32, bytes.len() as i32);
//     }
// }

// fn host_insert_row(id: i64, greeting: String) {
//     let call = HostCall::InsertRow(InsertRowRequest {
//         table_name: "greetings".to_string(),
//         row: Row {
//             primary_key: PrimitiveValue::I64(id),
//             entries: vec![PrimitiveValue::String(greeting)],
//         },
//     });

//     let bytes = encode(&call).unwrap();

//     unsafe {
//         interstice_host_call(bytes.as_ptr() as i32, bytes.len() as i32);
//     }
// }

// fn host_scan() -> Vec<Row> {
//     let call = HostCall::TableScan(TableScanRequest {
//         table: "greetings".to_string(),
//     });

//     let bytes = encode(&call).unwrap();

//     let pack = unsafe { interstice_host_call(bytes.as_ptr() as i32, bytes.len() as i32) };
//     let (ptr, len) = unpack_ptr_len(pack);
//     let bytes = unsafe { slice::from_raw_parts(ptr as *const u8, len as usize) };
//     let rows: Vec<Row> = decode(bytes).unwrap();
//     return rows;
// }

// REDUCERS

#[reducer]
pub fn hello(msg: String, test: u32) -> String {
    return format!("Hello, {}!", msg);
}

// #[no_mangle]
// pub extern "C" fn hello(ptr: i32, len: i32) -> i64 {
//     let bytes = unsafe { slice::from_raw_parts(ptr as *const u8, len as usize) };
//     let input: PrimitiveValue = decode(bytes).unwrap();

//     host_log("Previous greetings: ");
//     host_scan().iter().for_each(|row| {
//         if let PrimitiveValue::String(greeting) = &row.entries[0] {
//             host_log(&format!("- {}", greeting));
//         }
//     });

//     let mut msg = PrimitiveValue::String("Default Hello".to_string());
//     if let PrimitiveValue::String(name) = input {
//         msg = PrimitiveValue::String(format!("Hello, {}!", name));

//         host_log("Inserting greeting...");
//         host_insert_row(1, format!("Hello, {}!", name));
//     }

//     let bytes = encode(&msg).unwrap();
//     let out_ptr = alloc(bytes.len() as i32);
//     unsafe {
//         slice::from_raw_parts_mut(out_ptr as *mut u8, bytes.len()).copy_from_slice(&bytes);
//     }
//     return pack_ptr_len(out_ptr, bytes.len() as i32);
// }

// #[no_mangle]
// pub extern "C" fn on_greeting(ptr: i32, len: i32) -> i64 {
//     let bytes = unsafe { slice::from_raw_parts(ptr as *const u8, len as usize) };
//     let input: PrimitiveValue = decode(bytes).unwrap();

//     host_log("On new greetings: ");

//     let mut res = PrimitiveValue::Void;
//     let bytes = encode(&res).unwrap();
//     let out_ptr = alloc(bytes.len() as i32);
//     unsafe {
//         slice::from_raw_parts_mut(out_ptr as *mut u8, bytes.len()).copy_from_slice(&bytes);
//     }
//     return pack_ptr_len(out_ptr, bytes.len() as i32);
// }
