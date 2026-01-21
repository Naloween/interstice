use std::alloc::{alloc as local_alloc, dealloc as local_dealloc, Layout};
use std::slice;

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

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
pub extern "C" fn hello(ptr: i32, len: i32) -> i64 {
    let input = unsafe { slice::from_raw_parts(ptr as *const u8, len as usize) };

    let name = core::str::from_utf8(input).unwrap_or("world");

    let msg = format!("Hello, {}!", name);
    let bytes = msg.as_bytes();

    let out_ptr = alloc(bytes.len() as i32);
    unsafe {
        slice::from_raw_parts_mut(out_ptr as *mut u8, bytes.len()).copy_from_slice(bytes);
    }

    pack(out_ptr, bytes.len() as i32)
}
