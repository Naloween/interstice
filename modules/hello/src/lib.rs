use interstice_abi::{decode, encode, ModuleSchema, PrimitiveType, PrimitiveValue, ReducerSchema};
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

fn pack(ptr: i32, len: i32) -> i64 {
    ((ptr as i64) << 32) | (len as u32 as i64)
}

#[no_mangle]
pub extern "C" fn interstice_describe() -> i64 {
    let schema = ModuleSchema::new(
        "hello",
        1,
        vec![ReducerSchema::new(
            "hello",
            PrimitiveType::String,
            Some(PrimitiveType::String),
        )],
    );

    let bytes = encode(&schema).unwrap();

    let ptr = alloc(bytes.len() as i32);
    unsafe {
        core::slice::from_raw_parts_mut(ptr as *mut u8, bytes.len()).copy_from_slice(&bytes);
    }

    pack(ptr, bytes.len() as i32)
}

// REDUCERS

#[no_mangle]
pub extern "C" fn hello(ptr: i32, len: i32) -> i64 {
    let bytes = unsafe { slice::from_raw_parts(ptr as *const u8, len as usize) };
    let input: PrimitiveValue = decode(bytes).unwrap();

    if let PrimitiveValue::String(name) = input {
        let msg = PrimitiveValue::String(format!("Hello, {}!", name));
        let bytes = encode(&msg).unwrap();

        let out_ptr = alloc(bytes.len() as i32);
        unsafe {
            slice::from_raw_parts_mut(out_ptr as *mut u8, bytes.len()).copy_from_slice(&bytes);
        }
        return pack(out_ptr, bytes.len() as i32);
    }

    return pack(0, 0);
}
