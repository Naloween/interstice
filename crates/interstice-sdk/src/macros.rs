use interstice_abi::{
    Authority, ModuleDependency, ModuleVisibility, NodeDependency, encode, pack_ptr_len,
};

pub const fn validate_replicated_table_literal(value: &str) {
    let bytes = value.as_bytes();
    if bytes.is_empty() {
        panic!("Replicated table path cannot be empty");
    }

    let mut dot_count = 0;
    let mut segment_len = 0;
    let mut index = 0;

    while index < bytes.len() {
        let byte = bytes[index];

        if byte == b'.' {
            if segment_len == 0 {
                panic!("Replicated table path must not contain empty segments");
            }
            dot_count += 1;
            segment_len = 0;
        } else {
            let is_valid_char = (byte >= b'a' && byte <= b'z')
                || (byte >= b'A' && byte <= b'Z')
                || (byte >= b'0' && byte <= b'9')
                || byte == b'_'
                || byte == b'-';

            if !is_valid_char {
                panic!("Replicated table path contains unsupported characters");
            }
            segment_len += 1;
        }

        index += 1;
    }

    if dot_count != 2 || segment_len == 0 {
        panic!("Replicated table path must use 'node.module.table'");
    }
}

#[macro_export]
macro_rules! interstice_module {
    () => {
        interstice_module!(visibility: Private, authorities: [], replicated_tables: []);
    };

    (visibility: $vis:ident) => {
        interstice_module!(visibility: $vis, authorities: [], replicated_tables: []);
    };

    (authorities: [$($auth:ident),* $(,)?]) => {
        interstice_module!(visibility: Private, authorities: [$($auth),*], replicated_tables: []);
    };

    (replicated_tables: [$($rep:literal),* $(,)?]) => {
        interstice_module!(visibility: Private, authorities: [], replicated_tables: [$($rep),*]);
    };

    (visibility: $vis:ident, authorities: [$($auth:ident),* $(,)?]) => {
        interstice_module!(visibility: $vis, authorities: [$($auth),*], replicated_tables: []);
    };

    (visibility: $vis:ident, replicated_tables: [$($rep:literal),* $(,)?]) => {
        interstice_module!(visibility: $vis, authorities: [], replicated_tables: [$($rep),*]);
    };

    (authorities: [$($auth:ident),* $(,)?], replicated_tables: [$($rep:literal),* $(,)?]) => {
        interstice_module!(visibility: Private, authorities: [$($auth),*], replicated_tables: [$($rep),*]);
    };

    (visibility: $vis:ident, authorities: [$($auth:ident),* $(,)?], replicated_tables: [$($rep:literal),* $(,)?]) => {
        $(
            interstice_module!(@impl_authority $auth);
        )*

        // Global imports (for traits used in macros)
        use std::str::FromStr;

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

        // BINDINGS
        pub mod bindings {
            include!(concat!(env!("OUT_DIR"), "/interstice_bindings.rs"));
        }

        // Module Schema Description

        const __INTERSTICE_MODULE_NAME: &str = env!("CARGO_PKG_NAME");
        const __INTERSTICE_MODULE_VERSION: &str = env!("CARGO_PKG_VERSION");
        const __INTERSTICE_VISIBILITY: ModuleVisibility = ModuleVisibility::$vis;
        const __INTERSTICE_AUTHORITIES: &[interstice_abi::Authority] = &[
            $(interstice_abi::Authority::$auth),*
        ];

        #[unsafe(no_mangle)]
        pub extern "C" fn interstice_describe() -> i64 {
            let __interstice_replicated_tables: Vec<interstice_sdk::ReplicatedTableSchema> = vec![
                $(
                    {
                        const _: () = interstice_sdk::macros::validate_replicated_table_literal($rep);
                        let parts: Vec<&str> = $rep.split('.').collect();
                        interstice_sdk::ReplicatedTableSchema {
                            node_name: parts[0].to_string(),
                            module_name: parts[1].to_string(),
                            table_name: parts[2].to_string(),
                        }
                    }
                ),*
            ];

            let __interstice_node_dependencies = bindings::__GET_INTERSTICE_NODE_DEPENDENCIES();
            for table in &__interstice_replicated_tables {
                if let Err(error) = bindings::__INTERSTICE_VALIDATE_REPLICATED_TABLE(
                    &table.node_name,
                    &table.module_name,
                    &table.table_name,
                    &__interstice_node_dependencies,
                ) {
                    panic!("{}", error);
                }
            }

            interstice_sdk::macros::describe_module(
                __INTERSTICE_MODULE_NAME,
                __INTERSTICE_MODULE_VERSION,
                __INTERSTICE_VISIBILITY,
                __INTERSTICE_AUTHORITIES,
                bindings::__GET_INTERSTICE_MODULE_DEPENDENCIES(),
                __interstice_node_dependencies,
                __interstice_replicated_tables,
            )
        }

    };
    // Authorites calls

    (@impl_authority Input) => {
    };

    (@impl_authority Audio) => {
        pub trait AudioExt {
            fn audio(&self) -> Audio;
        }

        impl AudioExt for interstice_sdk::ReducerContext {
            fn audio(&self) -> interstice_sdk::Audio {
                interstice_sdk::Audio
            }
        }
    };

    (@impl_authority Gpu) => {
        pub trait GpuExt {
            fn gpu(&self) -> Gpu;
        }

        impl GpuExt for interstice_sdk::ReducerContext {
            fn gpu(&self) -> interstice_sdk::Gpu {
                interstice_sdk::Gpu
            }
        }
    };

    (@impl_authority File) => {
        pub trait FileExt {
            fn file(&self) -> File;
        }

        impl FileExt for interstice_sdk::ReducerContext {
            fn file(&self) -> interstice_sdk::File {
                interstice_sdk::File
            }
        }
    };

    (@impl_authority Module) => {
        pub trait ModuleExt {
            fn module(&self) -> ModuleAuthority;
        }

        impl ModuleExt for interstice_sdk::ReducerContext {
            fn module(&self) -> interstice_sdk::ModuleAuthority {
                interstice_sdk::ModuleAuthority
            }
        }
    };
}

pub fn describe_module(
    name: &str,
    version: &str,
    visibility: ModuleVisibility,
    authorities: &'static [Authority],
    module_dependencies: Vec<ModuleDependency>,
    node_dependencies: Vec<NodeDependency>,
    replicated_tables: Vec<interstice_abi::ReplicatedTableSchema>,
) -> i64 {
    let reducers = interstice_sdk_core::registry::collect_reducers();
    let queries = interstice_sdk_core::registry::collect_queries();
    let tables = interstice_sdk_core::registry::collect_tables();
    let subscriptions = interstice_sdk_core::registry::collect_subscriptions();
    let type_definitions = interstice_sdk_core::registry::collect_type_definitions();

    let schema = interstice_abi::ModuleSchema {
        abi_version: interstice_abi::ABI_VERSION,
        name: name.to_string(),
        version: version.into(),
        visibility,
        reducers,
        queries,
        tables,
        subscriptions,
        type_definitions,
        authorities: authorities.to_vec(),
        module_dependencies,
        node_dependencies,
        replicated_tables,
    };

    let bytes = encode(&schema).unwrap();
    let len = bytes.len() as i32;
    let ptr = Box::into_raw(bytes.into_boxed_slice()) as *mut u8 as i32;
    return pack_ptr_len(ptr, len);
}
