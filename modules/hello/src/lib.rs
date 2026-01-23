use interstice_abi::{IntersticeValue, Row};
use interstice_sdk::{interstice_module, reducer, table};
use std::alloc::{alloc as local_alloc, dealloc as local_dealloc, Layout};
use std::panic;

interstice_module!();

// #[no_mangle]
// pub extern "C" fn interstice_describe() -> i64 {
//     let schema = ModuleSchema::new(
//         "hello",
//         Version {
//             major: 0,
//             minor: 1,
//             patch: 0,
//         },
//         vec![
//             ReducerSchema::new(
//                 "hello",
//                 vec![EntrySchema {
//                     name: "name".to_string(),
//                     value_type: IntersticeType::String,
//                 }],
//                 IntersticeType::String,
//             ),
//             // ReducerSchema::new(
//             //     "on_greeting",
//             //     vec![EntrySchema {
//             //         name: "greeting".to_string(),
//             //         value_type: IntersticeType::String,
//             //     }],
//             //     IntersticeType::Void,
//             // ),
//         ],
//         vec![
//         //     TableSchema {
//         //     name: "greetings".to_string(),
//         //     visibility: TableVisibility::Public,
//         //     entries: vec![EntrySchema {
//         //         name: "greeting".to_string(),
//         //         value_type: IntersticeType::String,
//         //     }],
//         //     primary_key: EntrySchema {
//         //         name: "id".to_string(),
//         //         value_type: IntersticeType::I64,
//         //     },
//         // }
//         ],
//         vec![
//         //     SubscriptionSchema {
//         //     module_name: "hello".to_string(),
//         //     table_name: "greetings".to_string(),
//         //     reducer_name: "on_greeting".to_string(),
//         //     event: TableEvent::Insert,
//         // }
//         ],
//     );

//     let bytes = encode(&schema).unwrap();

//     let ptr = alloc(bytes.len() as i32);
//     unsafe {
//         core::slice::from_raw_parts_mut(ptr as *mut u8, bytes.len()).copy_from_slice(&bytes);
//     }

//     pack_ptr_len(ptr, bytes.len() as i32)
// }

// TABLES

#[table]
pub struct Greetings {
    #[primary_key]
    pub id: u64,
    pub greeting: String,
}

// REDUCERS

#[reducer]
pub fn hello(name: String) {
    interstice_sdk::log(&format!("Saying hello to {}", name));
    interstice_sdk::insert_row(
        "greetings".to_string(),
        Row {
            primary_key: IntersticeValue::U64(0), // Auto-incremented by the SDK
            entries: vec![IntersticeValue::String(format!("Hello, {}!", name))],
        },
    );
    let greetings = interstice_sdk::scan("greetings".into());
    interstice_sdk::log(&format!("Previous greetings: {:?}", greetings));
}

// #[no_mangle]
// pub extern "C" fn hello(ptr: i32, len: i32) -> i64 {
//     let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
//     let input: IntersticeValue = decode(bytes).unwrap();
//     let IntersticeValue::String(name) = input else {
//         panic!("Expected String argument");
//     };

//     let msg: IntersticeValue = format!("Hello, {}!", name).into();

//     let bytes = encode(&msg).unwrap();
//     let out_ptr = alloc(bytes.len() as i32);
//     unsafe {
//         std::slice::from_raw_parts_mut(out_ptr as *mut u8, bytes.len()).copy_from_slice(&bytes);
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
