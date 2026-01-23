use interstice_sdk::{interstice_module, reducer, table, IntersticeValue, Row};

interstice_module!();

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
}

#[reducer(on = hello.greetings.insert)]
fn on_greeting_insert(inserted_row: Row) {
    interstice_sdk::log(&format!("Inserted greeting: {:?}", inserted_row));

    let greetings = interstice_sdk::scan("greetings".into());
    interstice_sdk::log(&format!("All greetings: {:?}", greetings));
}

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
