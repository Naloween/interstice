use crate::{
    error::IntersticeError,
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
    ABI_VERSION, Authority, IntersticeValue, ModuleSchema, NodeSelection, ReducerContext,
    SubscriptionEventSchema, get_reducer_wrapper_name,
};
use serde::Serialize;
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};
use wasmtime::{Module as wasmtimeModule, Store};

pub struct Module {
    instance: Arc<Mutex<WasmInstance>>,
    pub schema: Arc<ModuleSchema>,
    pub tables: Arc<Mutex<HashMap<String, Table>>>,
}

impl Module {
    pub fn new(mut instance: WasmInstance) -> Result<Self, IntersticeError> {
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
    pub async fn load_module<P: AsRef<Path>>(
        runtime: Arc<Self>,
        path: P,
    ) -> Result<ModuleSchema, IntersticeError> {
        // Create wasm instance from provided file
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

        // Create module
        let module = Module::new(instance)?;
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
            println!(
                "Module '{}' is queued for loading after app initialization",
                runtime
                    .pending_app_modules
                    .lock()
                    .unwrap()
                    .last()
                    .unwrap()
                    .schema
                    .name
            );
            runtime.run_app_notify.notify_one();
            return Ok(module_schema.as_ref().clone());
        } else {
            Runtime::publish_module(runtime, module).await?;
            return Ok(module_schema.as_ref().clone());
        }
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

        runtime
            .modules
            .lock()
            .unwrap()
            .insert(module.schema.name.clone(), Arc::new(module));

        // Throw init event
        runtime
            .event_sender
            .send(EventInstance::Init {
                module_name: module_schema.name.clone(),
            })
            .unwrap();
        println!("Loaded module '{}'", module_schema.name);

        Ok(())
    }
}
