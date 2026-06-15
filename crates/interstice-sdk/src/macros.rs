use interstice_abi::{
    Authority, ModuleDependency, ModuleSelection, ModuleVisibility, NodeDependency, NodeSelection,
    QuerySchema, ReducerSchema, ReducerTableRef, ReplicatedTableSchema, SubscriptionEventSchema,
    SubscriptionSchema, encode, pack_ptr_len,
};
use std::collections::HashSet;

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

/// Install the module panic hook so guest panics are forwarded to the host log.
///
/// Idempotent and `Once`-guarded, so it is safe (and cheap) to call at the top of
/// every exported entry point. Installing here rather than from `.init_array`/ctors
/// avoids the "cannot modify the panic hook from a panicking thread" race: entry
/// points always run before any panic on that call, so `thread::panicking()` is false.
pub fn install_panic_hook() {
    static INSTALL_PANIC_HOOK: std::sync::Once = std::sync::Once::new();
    INSTALL_PANIC_HOOK.call_once(|| {
        if std::thread::panicking() {
            return;
        }
        std::panic::set_hook(Box::new(|info| {
            let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
                *s
            } else if let Some(s) = info.payload().downcast_ref::<String>() {
                s.as_str()
            } else {
                "panic occurred"
            };

            crate::host_calls::log(&format!("Panic Error: {}", msg));
        }));
    });
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
            interstice_sdk::macros::install_panic_hook();
            let layout = std::alloc::Layout::from_size_align(size as usize, 8).unwrap();
            unsafe { std::alloc::alloc(layout) as i32 }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn dealloc(ptr: i32, size: i32) {
            let layout = std::alloc::Layout::from_size_align(size as usize, 8).unwrap();
            unsafe { std::alloc::dealloc(ptr as *mut u8, layout) }
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
            interstice_sdk::macros::install_panic_hook();
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

    // Network is used through free host functions (tcp_connect, tcp_send, …),
    // so it needs no ctx extension trait — same shape as Input.
    (@impl_authority Network) => {
    };

    (@impl_authority Audio) => {
        pub trait AudioExt {
            fn audio(&self) -> Audio;
        }

        impl<Caps> AudioExt for interstice_sdk::ReducerContext<Caps> {
            fn audio(&self) -> interstice_sdk::Audio {
                interstice_sdk::Audio
            }
        }
    };

    (@impl_authority Gpu) => {
        pub trait GpuExt {
            fn gpu(&self) -> Gpu;
        }

        impl<Caps> GpuExt for interstice_sdk::ReducerContext<Caps> {
            fn gpu(&self) -> interstice_sdk::Gpu {
                interstice_sdk::Gpu
            }
        }
    };

    (@impl_authority File) => {
        pub trait FileExt {
            fn file(&self) -> File;
        }

        impl<Caps> FileExt for interstice_sdk::ReducerContext<Caps> {
            fn file(&self) -> interstice_sdk::File {
                interstice_sdk::File
            }
        }
    };

    (@impl_authority Module) => {
        pub trait ModuleExt {
            fn module(&self) -> ModuleAuthority;
        }

        impl<Caps> ModuleExt for interstice_sdk::ReducerContext<Caps> {
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

    validate_declared_accesses(
        name,
        &reducers,
        &queries,
        &tables,
        &module_dependencies,
        &node_dependencies,
        &replicated_tables,
    );

    validate_subscriptions(
        name,
        &subscriptions,
        &tables,
        &module_dependencies,
        &node_dependencies,
        &replicated_tables,
    );

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

fn validate_declared_accesses(
    current_module: &str,
    reducers: &[ReducerSchema],
    queries: &[QuerySchema],
    tables: &[interstice_abi::TableSchema],
    module_dependencies: &[ModuleDependency],
    node_dependencies: &[NodeDependency],
    replicated_tables: &[ReplicatedTableSchema],
) {
    let local_tables: HashSet<String> = tables.iter().map(|t| t.name.to_lowercase()).collect();
    let modules: HashSet<String> = module_dependencies
        .iter()
        .map(|m| m.module_name.to_lowercase())
        .collect();
    let nodes: HashSet<String> = node_dependencies
        .iter()
        .map(|n| n.name.to_lowercase())
        .collect();
    let replicas: HashSet<(String, String, String)> = replicated_tables
        .iter()
        .map(|r| {
            (
                r.node_name.to_lowercase(),
                r.module_name.to_lowercase(),
                r.table_name.to_lowercase(),
            )
        })
        .collect();

    for reducer in reducers {
        for access in &reducer.reads {
            validate_access_ref(
                current_module,
                reducer.name.as_str(),
                "reducer",
                "reads",
                access,
                &local_tables,
                &modules,
                &nodes,
                &replicas,
            );
        }
        for access in &reducer.inserts {
            validate_access_ref(
                current_module,
                reducer.name.as_str(),
                "reducer",
                "inserts",
                access,
                &local_tables,
                &modules,
                &nodes,
                &replicas,
            );
        }
        for access in &reducer.updates {
            validate_access_ref(
                current_module,
                reducer.name.as_str(),
                "reducer",
                "updates",
                access,
                &local_tables,
                &modules,
                &nodes,
                &replicas,
            );
        }
        for access in &reducer.deletes {
            validate_access_ref(
                current_module,
                reducer.name.as_str(),
                "reducer",
                "deletes",
                access,
                &local_tables,
                &modules,
                &nodes,
                &replicas,
            );
        }
    }

    for query in queries {
        for access in &query.reads {
            validate_access_ref(
                current_module,
                query.name.as_str(),
                "query",
                "reads",
                access,
                &local_tables,
                &modules,
                &nodes,
                &replicas,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_access_ref(
    current_module: &str,
    owner_name: &str,
    owner_kind: &str,
    op: &str,
    access: &ReducerTableRef,
    local_tables: &HashSet<String>,
    modules: &HashSet<String>,
    nodes: &HashSet<String>,
    replicas: &HashSet<(String, String, String)>,
) {
    let access_table_lc = access.table_name.to_lowercase();
    match (&access.node_selection, &access.module_selection) {
        (NodeSelection::Current, ModuleSelection::Current) => {
            if !local_tables.contains(&access_table_lc) {
                panic!(
                    "Invalid {owner_kind} `{owner_name}` {op} access: local table `{}` does not exist in module `{}`",
                    access.table_name, current_module
                );
            }
        }
        (NodeSelection::Current, ModuleSelection::Other(module)) => {
            if module == current_module {
                panic!(
                    "Invalid {owner_kind} `{owner_name}` {op} access: use current module table references for `{}`",
                    access.table_name
                );
            }
            if !modules.contains(&module.to_lowercase()) {
                panic!(
                    "Invalid {owner_kind} `{owner_name}` {op} access: module dependency `{module}` is not declared"
                );
            }
        }
        (NodeSelection::Other(node), ModuleSelection::Other(module)) => {
            if !nodes.contains(&node.to_lowercase()) {
                panic!(
                    "Invalid {owner_kind} `{owner_name}` {op} access: node dependency `{node}` is not declared"
                );
            }
            let key = (
                node.to_lowercase(),
                module.to_lowercase(),
                access_table_lc.clone(),
            );
            if !replicas.contains(&key) {
                panic!(
                    "Invalid {owner_kind} `{owner_name}` {op} access: replica `{node}.{module}.{}`
is not declared in `replicated_tables`",
                    access.table_name
                );
            }
        }
        (NodeSelection::Other(node), ModuleSelection::Current) => {
            panic!(
                "Invalid {owner_kind} `{owner_name}` {op} access: remote node `{node}` must use an explicit module selection"
            );
        }
    }
}

fn validate_subscriptions(
    current_module: &str,
    subscriptions: &[SubscriptionSchema],
    tables: &[interstice_abi::TableSchema],
    module_dependencies: &[ModuleDependency],
    node_dependencies: &[NodeDependency],
    replicated_tables: &[ReplicatedTableSchema],
) {
    let local_tables: HashSet<String> = tables.iter().map(|t| t.name.to_lowercase()).collect();
    let modules: HashSet<String> = module_dependencies
        .iter()
        .map(|m| m.module_name.to_lowercase())
        .collect();
    let nodes: HashSet<String> = node_dependencies
        .iter()
        .map(|n| n.name.to_lowercase())
        .collect();
    let replicas: HashSet<(String, String, String)> = replicated_tables
        .iter()
        .map(|r| {
            (
                r.node_name.to_lowercase(),
                r.module_name.to_lowercase(),
                r.table_name.to_lowercase(),
            )
        })
        .collect();

    for sub in subscriptions {
        match &sub.event {
            SubscriptionEventSchema::Insert {
                node_selection,
                module_name,
                table_name,
            }
            | SubscriptionEventSchema::Update {
                node_selection,
                module_name,
                table_name,
            }
            | SubscriptionEventSchema::Delete {
                node_selection,
                module_name,
                table_name,
            } => {
                let table_lc = table_name.to_lowercase();
                match node_selection {
                    NodeSelection::Current => {
                        if module_name == current_module {
                            if !local_tables.contains(&table_lc) {
                                panic!(
                                    "Invalid subscription `{}` event: local table `{}` does not exist in module `{}`",
                                    sub.reducer_name, table_name, current_module
                                );
                            }
                        } else {
                            if !modules.contains(&module_name.to_lowercase()) {
                                panic!(
                                    "Invalid subscription `{}` event: module dependency `{}` is not declared",
                                    sub.reducer_name, module_name
                                );
                            }
                        }
                    }
                    NodeSelection::Other(node) => {
                        if !nodes.contains(&node.to_lowercase()) {
                            panic!(
                                "Invalid subscription `{}` event: node dependency `{}` is not declared",
                                sub.reducer_name, node
                            );
                        }
                        let key = (
                            node.to_lowercase(),
                            module_name.to_lowercase(),
                            table_lc.clone(),
                        );
                        if !replicas.contains(&key) {
                            panic!(
                                "Invalid subscription `{}` event: replica `{}.{}. {}` is not declared in `replicated_tables`",
                                sub.reducer_name, node, module_name, table_name
                            );
                        }
                    }
                }
            }
            SubscriptionEventSchema::ReplicaSync {
                node_name,
                module_name,
                table_name,
            } => {
                if !node_dependencies
                    .iter()
                    .any(|n| n.name.to_lowercase() == node_name.to_lowercase())
                {
                    panic!(
                        "Invalid subscription `{}` event: node dependency `{}` is not declared",
                        sub.reducer_name, node_name
                    );
                }
                let key = (
                    node_name.to_lowercase(),
                    module_name.to_lowercase(),
                    table_name.to_lowercase(),
                );
                if !replicas.contains(&key) {
                    panic!(
                        "Invalid subscription `{}` event: replica `{}.{}. {}` is not declared in `replicated_tables`",
                        sub.reducer_name, node_name, module_name, table_name
                    );
                }
            }
            _ => {}
        }
    }
}
