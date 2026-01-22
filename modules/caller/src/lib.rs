use interstice_abi::{
    codec::pack_ptr_len, encode, CallReducerRequest, HostCall, LogRequest, ModuleSchema,
    PrimitiveType, PrimitiveValue, ReducerSchema,
};
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
        "caller",
        1,
        vec![ReducerSchema::new("caller", vec![], PrimitiveType::Void)],
        vec![],
        vec![],
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

fn host_call(target_module: String, reducer: String, input: PrimitiveValue) {
    let call = HostCall::CallReducer(CallReducerRequest {
        target_module,
        reducer,
        input,
    });

    let bytes = encode(&call).unwrap();

    unsafe {
        interstice_host_call(bytes.as_ptr() as i32, bytes.len() as i32);
    }
}

// REDUCERS

#[no_mangle]
pub extern "C" fn caller(_ptr: i32, _len: i32) -> i64 {
    host_log("Calling hello....");
    host_call(
        "hello".to_string(),
        "hello".to_string(),
        PrimitiveValue::String("called from caller".to_string()),
    );
    host_log("hello called !");

    let bytes = encode(&PrimitiveValue::Void).unwrap();
    let out_ptr = alloc(bytes.len() as i32);
    unsafe {
        slice::from_raw_parts_mut(out_ptr as *mut u8, bytes.len()).copy_from_slice(&bytes);
    }
    return pack_ptr_len(out_ptr, bytes.len() as i32);
}
