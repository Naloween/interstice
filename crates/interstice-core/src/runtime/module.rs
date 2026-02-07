use crate::{
    error::IntersticeError,
    logger::{LogLevel, LogSource},
    network::protocol::{NetworkPacket, RequestSubscription, TableEvent},
    runtime::{
        Runtime,
        authority::AuthorityEntry,
        event::EventInstance,
        table::Table,
        wasm::{StoreState, instance::WasmInstance},
    },
};
use interstice_abi::{
    ABI_VERSION, Authority, FileEvent, IntersticeValue, ModuleSchema, NodeSelection,
    ReducerContext, SubscriptionEventSchema, get_reducer_wrapper_name,
};
use notify::{RecursiveMode, Watcher};
use serde::Serialize;
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};
use wasmtime::{Module as wasmtimeModule, Store};

pub struct Module {
    instance: Arc<Mutex<WasmInstance>>,
    wasm_bytes: Vec<u8>,
    pub schema: Arc<ModuleSchema>,
    pub tables: Arc<Mutex<HashMap<String, Table>>>,
}

impl Module {
    pub fn from_file(runtime: Arc<Runtime>, path: &Path) -> Result<Self, IntersticeError> {
        let wasm_module = wasmtimeModule::from_file(&runtime.engine, path).unwrap();
        let mut store = Store::new(
            &runtime.engine,
            StoreState {
                runtime: runtime.clone(),
                module_schema: ModuleSchema::empty(),
            },
        );
        let instance = runtime
            .linker
            .lock()
            .unwrap()
            .instantiate(&mut store, &wasm_module)
            .unwrap();
        let instance = WasmInstance::new(store, instance)?;
        let module = Module::new(instance, std::fs::read(path).unwrap())?;
        Ok(module)
    }

    pub fn from_bytes(runtime: Arc<Runtime>, wasm_binary: &[u8]) -> Result<Self, IntersticeError> {
        let wasm_module = wasmtimeModule::new(&runtime.engine, wasm_binary).unwrap();
        let mut store = Store::new(
            &runtime.engine,
            StoreState {
                runtime: runtime.clone(),
                module_schema: ModuleSchema::empty(),
            },
        );
        let instance = runtime
            .linker
            .lock()
            .unwrap()
            .instantiate(&mut store, &wasm_module)
            .unwrap();
        let instance = WasmInstance::new(store, instance)?;
        let module = Module::new(instance, wasm_binary.to_vec())?;
        Ok(module)
    }

    pub fn new(mut instance: WasmInstance, wasm_bytes: Vec<u8>) -> Result<Self, IntersticeError> {
        let schema = instance.load_schema()?;
        if schema.abi_version != ABI_VERSION {
            return Err(IntersticeError::AbiVersionMismatch {
                expected: ABI_VERSION,
                found: schema.abi_version,
            });
        }

        let tables = schema
            .tables
            .iter()
            .map(|table_schema| {
                (
                    table_schema.name.clone(),
                    Table {
                        schema: table_schema.clone(),
                        rows: Vec::new(),
                    },
                )
            })
            .collect();

        // Set module name in the store state
        instance.store.data_mut().module_schema = schema.clone();

        Ok(Self {
            instance: Arc::new(Mutex::new(instance)),
            wasm_bytes,
            schema: Arc::new(schema),
            tables: Arc::new(Mutex::new(tables)),
        })
    }

    pub fn call_reducer(
        &self,
        reducer: &str,
        args: (ReducerContext, impl Serialize),
    ) -> Result<IntersticeValue, IntersticeError> {
        let func_name = &get_reducer_wrapper_name(reducer);
        return self.instance.lock().unwrap().call_function(func_name, args);
    }
}

impl Runtime {
    pub async fn load_module(
        runtime: Arc<Self>,
        module: Module,
    ) -> Result<ModuleSchema, IntersticeError> {
        let module_schema = module.schema.clone();

        // If the module requires GPU authority and the app is not initialized yet, queue it for loading after app initialization
        if module_schema
            .authorities
            .iter()
            .find(|a| *a == &Authority::Gpu)
            .is_some()
            && !*runtime.app_initialized.lock().unwrap()
        {
            runtime.pending_app_modules.lock().unwrap().push(module);
            runtime.logger.log(
                &format!(
                    "Module '{}' is queued for loading after app initialization",
                    runtime
                        .pending_app_modules
                        .lock()
                        .unwrap()
                        .last()
                        .unwrap()
                        .schema
                        .name
                ),
                LogSource::Runtime,
                LogLevel::Info,
            );
            runtime.run_app_notify.notify_one();
            return Ok(module_schema.as_ref().clone());
        }
        Runtime::publish_module(runtime, module).await?;
        return Ok(module_schema.as_ref().clone());
    }

    pub async fn publish_module(runtime: Arc<Self>, module: Module) -> Result<(), IntersticeError> {
        let module_schema = module.schema.clone();
        for authority in &module_schema.authorities {
            if let Some(other_entry) = runtime.authority_modules.lock().unwrap().get(authority) {
                return Err(IntersticeError::AuthorityAlreadyTaken(
                    module_schema.name.clone(),
                    authority.clone().into(),
                    other_entry.module_name.clone(),
                ));
            } else {
                let on_event_reducer_name = match authority {
                    interstice_abi::Authority::Gpu => module_schema
                        .subscriptions
                        .iter()
                        .find(|sub| sub.event == SubscriptionEventSchema::Render)
                        .map(|sub| sub.reducer_name.clone()),
                    interstice_abi::Authority::Audio => None,
                    interstice_abi::Authority::Input => module_schema
                        .subscriptions
                        .iter()
                        .find(|sub| sub.event == SubscriptionEventSchema::Input)
                        .map(|sub| sub.reducer_name.clone()),
                    interstice_abi::Authority::File => None,
                };
                runtime.authority_modules.lock().unwrap().insert(
                    authority.clone(),
                    AuthorityEntry {
                        module_name: module_schema.name.clone(),
                        on_event_reducer_name,
                    },
                );
            }
        }

        // Check name
        if runtime
            .modules
            .lock()
            .unwrap()
            .contains_key(&module.schema.name)
        {
            return Err(IntersticeError::ModuleAlreadyExists(
                module.schema.name.clone(),
            ));
        }

        // Check dependencies
        for dependency in &module.schema.module_dependencies {
            if let Some(dependency_module) =
                runtime.modules.lock().unwrap().get(&dependency.module_name)
            {
                if dependency_module.schema.version != dependency.version {
                    return Err(IntersticeError::ModuleVersionMismatch(
                        module.schema.name.clone(),
                        dependency_module.schema.name.clone(),
                        module.schema.version.clone(),
                        dependency_module.schema.version.clone(),
                    ));
                }
            } else {
                return Err(IntersticeError::ModuleNotFound(
                    dependency.module_name.clone(),
                    format!(
                        "Required by '{}' which depends on it",
                        module.schema.name.clone()
                    ),
                ));
            }
        }

        // Connect to node dependencies
        for node_dependency in &module_schema.node_dependencies {
            let network = &mut runtime.network_handle.clone();
            network
                .connect_to_peer(node_dependency.address.clone())
                .await;
        }

        // Send subscription requests to remote subscriptions
        let network = &mut runtime.network_handle.clone();
        for sub in &module_schema.subscriptions {
            match sub.event.clone() {
                SubscriptionEventSchema::Insert {
                    node_selection: NodeSelection::Other(node_name),
                    module_name,
                    table_name,
                }
                | SubscriptionEventSchema::Update {
                    node_selection: NodeSelection::Other(node_name),
                    module_name,
                    table_name,
                }
                | SubscriptionEventSchema::Delete {
                    node_selection: NodeSelection::Other(node_name),
                    module_name,
                    table_name,
                } => {
                    let node_adress = module_schema
                        .node_dependencies
                        .iter()
                        .find(|n| n.name == node_name)
                        .ok_or(IntersticeError::Internal(format!(
                            "Couldn't find node {node_name} in the node dependencies"
                        )))?
                        .address
                        .clone();
                    let node_id = network.get_node_id_from_adress(&node_adress).unwrap();
                    network.send_packet(
                        node_id,
                        NetworkPacket::RequestSubscription(RequestSubscription {
                            module_name,
                            table_name,
                            event: match sub.event {
                                SubscriptionEventSchema::Insert { .. } => TableEvent::Insert,
                                SubscriptionEventSchema::Update { .. } => TableEvent::Update,
                                SubscriptionEventSchema::Delete { .. } => TableEvent::Delete,
                                _ => unreachable!(),
                            },
                        }),
                    );
                }
                _ => {}
            }
        }

        // save module
        let module_path = runtime
            .modules_path
            .join(format!("{}.wasm", module_schema.name));
        std::fs::write(module_path, &module.wasm_bytes).unwrap();

        runtime
            .modules
            .lock()
            .unwrap()
            .insert(module.schema.name.clone(), Arc::new(module));

        setup_file_watches(runtime.clone(), &module_schema)?;

        // Throw init event
        runtime
            .event_sender
            .send(EventInstance::Init {
                module_name: module_schema.name.clone(),
            })
            .unwrap();
        runtime.logger.log(
            &format!("Loaded module '{}'", module_schema.name),
            LogSource::Runtime,
            LogLevel::Info,
        );

        Ok(())
    }

    pub fn remove_module(runtime: Arc<Runtime>, module_name: &str) {
        runtime.modules.lock().unwrap().remove(module_name);
        let module_path = runtime.modules_path.join(format!("{}.wasm", module_name));
        // Removing module file
        if module_path.exists() {
            std::fs::remove_file(module_path).unwrap();
        }
        // Closing module network connections
        let node_ids_to_disconnect = runtime
            .network_handle
            .connected_peers()
            .into_iter()
            .filter(|(node_id, _)| {
                runtime
                    .node_subscriptions
                    .lock()
                    .unwrap()
                    .get(node_id)
                    .map(|subs| {
                        subs.iter().any(|sub| match sub {
                            SubscriptionEventSchema::Insert {
                                module_name: sub_module_name,
                                ..
                            }
                            | SubscriptionEventSchema::Update {
                                module_name: sub_module_name,
                                ..
                            }
                            | SubscriptionEventSchema::Delete {
                                module_name: sub_module_name,
                                ..
                            } => sub_module_name == module_name,
                            _ => false,
                        })
                    })
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();

        for (node_id, _) in node_ids_to_disconnect {
            let network_handle = runtime.network_handle.clone();
            tokio::spawn(async move {
                network_handle.disconnect_peer(node_id).await;
            });
        }

        // Removing module from authority modules if it has any authority
        let authorities_to_remove = runtime
            .authority_modules
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, entry)| entry.module_name == module_name)
            .map(|(authority, _)| authority.clone())
            .collect::<Vec<_>>();
        for authority in authorities_to_remove {
            runtime.authority_modules.lock().unwrap().remove(&authority);
        }

        runtime.file_watchers.lock().unwrap().clear();

        runtime.logger.log(
            &format!("Removed module '{}'", module_name),
            LogSource::Runtime,
            LogLevel::Info,
        );
    }
}

fn setup_file_watches(
    runtime: Arc<Runtime>,
    module_schema: &ModuleSchema,
) -> Result<(), IntersticeError> {
    if !module_schema
        .authorities
        .iter()
        .any(|a| *a == Authority::File)
    {
        return Ok(());
    }

    let mut watchers_for_module = Vec::new();

    for sub in &module_schema.subscriptions {
        if let SubscriptionEventSchema::File { path, recursive } = &sub.event {
            let event_sender = runtime.event_sender.clone();
            let logger = runtime.logger.clone();

            let mut watcher =
                notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
                    match res {
                        Ok(event) => {
                            let mut events = Vec::new();
                            match event.kind {
                                notify::EventKind::Create(_) => {
                                    for p in &event.paths {
                                        events.push(FileEvent::Created {
                                            path: p.to_string_lossy().to_string(),
                                        });
                                    }
                                }
                                notify::EventKind::Modify(notify::event::ModifyKind::Name(_)) => {
                                    if event.paths.len() >= 2 {
                                        events.push(FileEvent::Renamed {
                                            from: event.paths[0].to_string_lossy().to_string(),
                                            to: event.paths[1].to_string_lossy().to_string(),
                                        });
                                    }
                                }
                                notify::EventKind::Modify(_) => {
                                    for p in &event.paths {
                                        events.push(FileEvent::Modified {
                                            path: p.to_string_lossy().to_string(),
                                        });
                                    }
                                }
                                notify::EventKind::Remove(_) => {
                                    for p in &event.paths {
                                        events.push(FileEvent::Deleted {
                                            path: p.to_string_lossy().to_string(),
                                        });
                                    }
                                }
                                _ => {}
                            }

                            for ev in events {
                                let _ = event_sender.send(EventInstance::File(ev));
                            }
                        }
                        Err(err) => {
                            logger.log(
                                &format!("File watch error: {}", err),
                                LogSource::Runtime,
                                LogLevel::Warning,
                            );
                        }
                    }
                })
                .map_err(|err| {
                    IntersticeError::Internal(format!("Failed to create watcher: {}", err))
                })?;

            let mode = if *recursive {
                RecursiveMode::Recursive
            } else {
                RecursiveMode::NonRecursive
            };

            watcher.watch(Path::new(path), mode).map_err(|err| {
                IntersticeError::Internal(format!("Failed to watch path: {}", err))
            })?;

            watchers_for_module.push(watcher);
        }
    }

    if !watchers_for_module.is_empty() {
        runtime
            .file_watchers
            .lock()
            .unwrap()
            .extend(watchers_for_module);
    }

    Ok(())
}
