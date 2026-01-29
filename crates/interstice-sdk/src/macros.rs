use interstice_abi::{Authority, encode, pack_ptr_len};

#[macro_export]
macro_rules! interstice_module {
    () => {
        interstice_module!(None);
    };
    ($authority:expr) => {
        // Global imports (for traits used in macros)
        use std::str::FromStr;
        // Use wee_alloc as the global allocator.

        #[global_allocator]
        static ALLOC: interstice_sdk::wee_alloc::WeeAlloc =
            interstice_sdk::wee_alloc::WeeAlloc::INIT;

        #[unsafe(no_mangle)]
        pub extern "C" fn alloc(size: i32) -> i32 {
            let layout = std::alloc::Layout::from_size_align(size as usize, 8).unwrap();
            unsafe { std::alloc::alloc(layout) as i32 }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn dealloc(ptr: i32, size: i32) {
            let layout = std::alloc::Layout::from_size_align(size as usize, 8).unwrap();
            unsafe { std::alloc::dealloc(ptr as *mut u8, layout) }
        }

        // Panic hook to log panics to host
        #[$crate::init]
        fn interstice_init() {
            std::panic::set_hook(Box::new(|info| {
                let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
                    *s
                } else if let Some(s) = info.payload().downcast_ref::<String>() {
                    s.as_str()
                } else {
                    "panic occurred"
                };

                // send to host
                interstice_sdk::host_calls::log(&format!("Panic Error: {}", msg));
            }));
        }

        // Module Schema Description

        const __INTERSTICE_MODULE_NAME: &str = env!("CARGO_PKG_NAME");
        const __INTERSTICE_MODULE_VERSION: &str = env!("CARGO_PKG_VERSION");

        #[unsafe(no_mangle)]
        pub extern "C" fn interstice_describe() -> i64 {
            interstice_sdk::macros::describe_module(
                __INTERSTICE_MODULE_NAME,
                __INTERSTICE_MODULE_VERSION,
                $authority,
            )
        }

        // BINDINGS
        pub mod bindings {
            include!(concat!(env!("OUT_DIR"), "/interstice_bindings.rs"));
        }
    };
}

pub fn describe_module(name: &str, version: &str, authority: Option<Authority>) -> i64 {
    let reducers = interstice_sdk_core::registry::collect_reducers();
    let tables = interstice_sdk_core::registry::collect_tables();
    let subscriptions = interstice_sdk_core::registry::collect_subscriptions();
    let type_definitions = interstice_sdk_core::registry::collect_type_definitions();

    let schema = interstice_abi::ModuleSchema {
        abi_version: interstice_abi::ABI_VERSION,
        name: name.to_string(),
        version: version.into(),
        reducers,
        tables,
        subscriptions,
        type_definitions,
        authority,
    };

    let bytes = encode(&schema).unwrap();
    let len = bytes.len() as i32;
    let ptr = Box::into_raw(bytes.into_boxed_slice()) as *mut u8 as i32;
    return pack_ptr_len(ptr, len);
}
