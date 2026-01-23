#[macro_export]
macro_rules! interstice_module {
    () => {
        #[global_allocator]
        static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

        #[unsafe(no_mangle)]
        pub extern "C" fn alloc(size: i32) -> i32 {
            let layout = Layout::from_size_align(size as usize, 8).unwrap();
            unsafe { local_alloc(layout) as i32 }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn dealloc(ptr: i32, size: i32) {
            let layout = Layout::from_size_align(size as usize, 8).unwrap();
            unsafe { local_dealloc(ptr as *mut u8, layout) }
        }

        const __INTERSTICE_MODULE_NAME: &str = env!("CARGO_PKG_NAME");
        const __INTERSTICE_MODULE_VERSION: &str = env!("CARGO_PKG_VERSION");

        #[unsafe(no_mangle)]
        pub extern "C" fn interstice_describe() -> i64 {
            interstice_sdk::internal::describe_module(
                __INTERSTICE_MODULE_NAME,
                __INTERSTICE_MODULE_VERSION,
            )
        }
    };
}
