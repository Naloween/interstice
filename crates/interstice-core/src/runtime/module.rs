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
    ABI_VERSION, Authority, FileEvent, IndexKey, IntersticeValue, ModuleSchema, NodeSchema,
    NodeSelection, QueryContext, ReducerContext, SubscriptionEventSchema, TableVisibility,
    get_query_wrapper_name, get_reducer_wrapper_name,
};
use notify::{RecursiveMode, Watcher};
use serde::Serialize;
use std::convert::TryInto;
use parking_lot::Mutex;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::Arc,
};
use tokio::time::{Duration, timeout};
use uuid::Uuid;
use wasmtime::{Module as wasmtimeModule, Store};

/// Number of WASM instances kept in the reducer pool. More instances allow more concurrent
/// reducer executions at the cost of extra WASM linear memory per module (TLB pressure).
const REDUCER_POOL_SIZE: usize = 8;

pub struct Module {
    /// Pool of WASM instances for reducer execution. Multiple threads draw from this pool,
    /// enabling parallel reducer execution for independent reducers.
    instance_pool: Arc<(parking_lot::Mutex<Vec<WasmInstance>>, parking_lot::Condvar)>,
    query_instance: Arc<Mutex<WasmInstance>>,
    wasm_bytes: Vec<u8>,
    pub schema: Arc<ModuleSchema>,
    pub tables: Arc<Mutex<HashMap<String, Table>>>,
    pub reducer_names: HashSet<String>,
}

impl std::fmt::Debug for Module {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Module")
            .field("schema", &self.schema.name)
            .finish_non_exhaustive()
    }
}

impl Module {
    pub async fn from_file(runtime: Arc<Runtime>, path: &Path) -> Result<Self, IntersticeError> {
        let wasm_bytes = std::fs::read(path).unwrap();
        let wasm_module = wasmtimeModule::from_file(&runtime.engine, path).unwrap();

        let mut instances = Vec::with_capacity(REDUCER_POOL_SIZE);
        for _ in 0..REDUCER_POOL_SIZE {
            let mut store = Store::new(
                &runtime.engine,
                StoreState {
                    runtime: runtime.clone(),
                    module_schema: ModuleSchema::empty(),
                },
            );
            let raw = runtime.linker.instantiate(&mut store, &wasm_module).unwrap();
            instances.push(WasmInstance::new(store, raw)?);
        }

        let mut query_store = Store::new(
            &runtime.engine,
            StoreState {
                runtime: runtime.clone(),
                module_schema: ModuleSchema::empty(),
            },
        );
        let query_raw = runtime
            .linker
            .instantiate(&mut query_store, &wasm_module)
            .unwrap();
        let query_instance = WasmInstance::new(query_store, query_raw)?;

        let module = Module::new(instances, query_instance, wasm_bytes).await?;
        Ok(module)
    }

    pub async fn from_bytes(
        runtime: Arc<Runtime>,
        wasm_binary: &[u8],
    ) -> Result<Self, IntersticeError> {
        let wasm_module = wasmtimeModule::new(&runtime.engine, wasm_binary).unwrap();

        let mut instances = Vec::with_capacity(REDUCER_POOL_SIZE);
        for _ in 0..REDUCER_POOL_SIZE {
            let mut store = Store::new(
                &runtime.engine,
                StoreState {
                    runtime: runtime.clone(),
                    module_schema: ModuleSchema::empty(),
                },
            );
            let raw = runtime.linker.instantiate(&mut store, &wasm_module).unwrap();
            instances.push(WasmInstance::new(store, raw)?);
        }

        let mut query_store = Store::new(
            &runtime.engine,
            StoreState {
                runtime: runtime.clone(),
                module_schema: ModuleSchema::empty(),
            },
        );
        let query_raw = runtime
            .linker
            .instantiate(&mut query_store, &wasm_module)
            .unwrap();
        let query_instance = WasmInstance::new(query_store, query_raw)?;

        let module = Module::new(instances, query_instance, wasm_binary.to_vec()).await?;
        Ok(module)
    }

    pub async fn new(
        mut instances: Vec<WasmInstance>,
        mut query_instance: WasmInstance,
        wasm_bytes: Vec<u8>,
    ) -> Result<Self, IntersticeError> {
        assert!(!instances.is_empty(), "at least one reducer instance required");
        let schema = instances[0].load_schema()?;
        if schema.abi_version != ABI_VERSION {
            return Err(IntersticeError::AbiVersionMismatch {
                expected: ABI_VERSION,
                found: schema.abi_version,
            });
        }

        let tables = schema
            .tables
            .iter()
            .map(|table_schema| (table_schema.name.clone(), Table::new(table_schema.clone())))
            .collect();

        let reducer_names = schema.reducers.iter().map(|r| r.name.clone()).collect();

        // Pre-cache func handles and allocate scratch buffers in every pool instance.
        let reducer_func_names: Vec<String> = schema
            .reducers
            .iter()
            .map(|r| get_reducer_wrapper_name(&r.name))
            .collect();
        for inst in &mut instances {
            inst.store.data_mut().module_schema = schema.clone();
            inst.preload_funcs(&reducer_func_names);
            inst.init_scratch(4096).ok();
        }

        let query_func_names: Vec<String> = schema
            .queries
            .iter()
            .map(|q| get_query_wrapper_name(&q.name))
            .collect();
        query_instance.store.data_mut().module_schema = schema.clone();
        query_instance.preload_funcs(&query_func_names);
        query_instance.init_scratch(4096).ok();

        let instance_pool = Arc::new((
            parking_lot::Mutex::new(instances),
            parking_lot::Condvar::new(),
        ));

        Ok(Self {
            instance_pool,
            query_instance: Arc::new(Mutex::new(query_instance)),
            wasm_bytes,
            schema: Arc::new(schema),
            tables: Arc::new(Mutex::new(tables)),
            reducer_names,
        })
    }

    pub fn call_reducer(
        &self,
        reducer: &str,
        args: (ReducerContext, impl Serialize),
    ) -> Result<(), IntersticeError> {
        let func_name = get_reducer_wrapper_name(reducer);
        // Borrow an instance from the pool; block if all are in use.
        let (lock, cvar) = &*self.instance_pool;
        let mut instance = {
            let mut pool = lock.lock();
            while pool.is_empty() {
                cvar.wait(&mut pool);
            }
            pool.pop().unwrap()
        };
        let result = instance.call_reducer(&func_name, args);
        // Return the instance to the pool and wake a waiting thread if any.
        {
            let mut pool = lock.lock();
            pool.push(instance);
            cvar.notify_one();
        }
        result
    }

    pub fn call_query(
        &self,
        query: &str,
        args: (QueryContext, impl Serialize),
    ) -> Result<IntersticeValue, IntersticeError> {
        let func_name = get_query_wrapper_name(query);
        self.query_instance.lock().call_query(&func_name, args)
    }
}

impl Runtime {
    pub async fn load_module(
        runtime: Arc<Self>,
        module: Module,
        fire_init: bool,
    ) -> Result<ModuleSchema, IntersticeError> {
        let module_schema = module.schema.clone();

        // If the module requires GPU authority and the app is not initialized yet, initialize it
        if module_schema
            .authorities
            .iter()
            .find(|a| *a == &Authority::Gpu)
            .is_some()
            && !*runtime.app_initialized.lock()
        {
            runtime.logger.log(
                &format!(
                    "Module '{}' requested app initialization",
                    module_schema.name
                ),
                LogSource::Runtime,
                LogLevel::Info,
            );
            runtime
                .event_sender
                .send((EventInstance::RequestAppInitialization, None))
                .expect("Couldn't send requets app initialization event");
        }
        for authority in &module_schema.authorities {
            if let Some(other_entry) = runtime.authority_modules.lock().get(authority) {
                return Err(IntersticeError::AuthorityAlreadyTaken(
                    module_schema.name.clone(),
                    authority.clone().into(),
                    other_entry.module_name().to_string(),
                ));
            } else {
                let module_name = module_schema.name.clone();
                let entry = match authority {
                    Authority::Gpu => AuthorityEntry::Gpu {
                        module_name,
                        render_reducer: module_schema
                            .subscriptions
                            .iter()
                            .find(|sub| sub.event == SubscriptionEventSchema::Render)
                            .map(|sub| sub.reducer_name.clone()),
                    },
                    Authority::Audio => AuthorityEntry::Audio {
                        module_name,
                        output_reducer: module_schema
                            .subscriptions
                            .iter()
                            .find(|sub| sub.event == SubscriptionEventSchema::AudioOutput)
                            .map(|sub| sub.reducer_name.clone()),
                        input_reducer: module_schema
                            .subscriptions
                            .iter()
                            .find(|sub| sub.event == SubscriptionEventSchema::AudioInput)
                            .map(|sub| sub.reducer_name.clone()),
                    },
                    Authority::Input => AuthorityEntry::Input {
                        module_name,
                        input_reducer: module_schema
                            .subscriptions
                            .iter()
                            .find(|sub| sub.event == SubscriptionEventSchema::Input)
                            .map(|sub| sub.reducer_name.clone()),
                    },
                    Authority::File => AuthorityEntry::File { module_name },
                    Authority::Module => AuthorityEntry::Module { module_name },
                };
                runtime
                    .authority_modules
                    .lock()
                    
                    .insert(authority.clone(), entry);
            }
        }

        // Check name
        if runtime
            .modules
            .lock()
            
            .contains_key(&module.schema.name)
        {
            return Err(IntersticeError::ModuleAlreadyExists(
                module.schema.name.clone(),
            ));
        }

        // Check dependencies
        for dependency in &module.schema.module_dependencies {
            if let Some(dependency_module) =
                runtime.modules.lock().get(&dependency.module_name)
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

            if let Ok(node_id) = network.get_node_id_from_adress(&node_dependency.address) {
                runtime
                    .node_names_by_id
                    .lock()
                    
                    .insert(node_id, node_dependency.name.clone());
            }
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

        // Configure explicit replicated remote public tables
        let mut remote_node_schemas: HashMap<String, NodeSchema> = HashMap::new();
        for replicated in &module_schema.replicated_tables {
            let node_dependency = module_schema
                .node_dependencies
                .iter()
                .find(|n| n.name == replicated.node_name)
                .ok_or_else(|| {
                    IntersticeError::Internal(format!(
                        "Replicated table '{}.{}.{}' requires node '{}' in dependencies",
                        replicated.node_name,
                        replicated.module_name,
                        replicated.table_name,
                        replicated.node_name
                    ))
                })?;

            let node_id = runtime
                .network_handle
                .get_node_id_from_adress(&node_dependency.address)
                .map_err(|_| {
                    IntersticeError::Internal(format!(
                        "Couldn't resolve node id for node dependency '{}' while setting replicas",
                        replicated.node_name
                    ))
                })?;

            let node_schema = if let Some(schema) = remote_node_schemas.get(&replicated.node_name) {
                schema.clone()
            } else {
                let schema = runtime
                    .request_node_schema(node_id, replicated.node_name.clone())
                    .await?;
                remote_node_schemas.insert(replicated.node_name.clone(), schema.clone());
                schema
            };

            let remote_module = node_schema
                .modules
                .iter()
                .find(|m| m.name == replicated.module_name)
                .ok_or_else(|| {
                    IntersticeError::ModuleNotFound(
                        replicated.module_name.clone(),
                        format!(
                            "Remote module '{}' not found on node '{}' while configuring table replica",
                            replicated.module_name, replicated.node_name
                        ),
                    )
                })?;

            let remote_table = remote_module
                .tables
                .iter()
                .find(|t| t.name == replicated.table_name)
                .ok_or_else(|| IntersticeError::TableNotFound {
                    module_name: replicated.module_name.clone(),
                    table_name: replicated.table_name.clone(),
                })?;

            if remote_table.visibility != TableVisibility::Public {
                return Err(IntersticeError::Internal(format!(
                    "Cannot replicate non-public table '{}.{}.{}'",
                    replicated.node_name, replicated.module_name, replicated.table_name
                )));
            }

            let local_table_name = format!(
                "__replica__{}__{}__{}",
                replicated.node_name.replace('-', "_").replace('.', "_"),
                replicated.module_name.replace('-', "_").replace('.', "_"),
                replicated.table_name.replace('-', "_").replace('.', "_"),
            );

            let mut local_schema = remote_table.clone();
            local_schema.name = local_table_name.clone();
            module
                .tables
                .lock()
                
                .insert(local_table_name.clone(), Table::new(local_schema));

            runtime
                .replica_bindings
                .lock()
                
                .push(crate::runtime::ReplicaBinding {
                    owner_module_name: module_schema.name.clone(),
                    source_node_id: node_id,
                    source_node_name: replicated.node_name.clone(),
                    source_module_name: replicated.module_name.clone(),
                    source_table_name: replicated.table_name.clone(),
                    local_table_name,
                });
        }

        // save module
        if let Some(modules_path) = &runtime.modules_path {
            let module_dir = modules_path.join(&module_schema.name);
            std::fs::create_dir_all(&module_dir).unwrap();
            std::fs::create_dir_all(module_dir.join("logs")).unwrap();
            std::fs::create_dir_all(module_dir.join("snapshots")).unwrap();
            std::fs::write(module_dir.join("module.wasm"), &module.wasm_bytes).unwrap();
        }

        // Count table subscriptions (Insert/Update/Delete) and add to active_subscription_count
        let table_sub_count = module_schema
            .subscriptions
            .iter()
            .filter(|s| matches!(
                s.event,
                SubscriptionEventSchema::Insert { .. }
                    | SubscriptionEventSchema::Update { .. }
                    | SubscriptionEventSchema::Delete { .. }
            ))
            .count() as i32;
        if table_sub_count > 0 {
            runtime
                .active_subscription_count
                .fetch_add(table_sub_count, std::sync::atomic::Ordering::Relaxed);
        }

        runtime
            .modules
            .lock()
            
            .insert(module.schema.name.clone(), Arc::new(module));
        runtime.clear_reducer_access_cache();

        // Now that the module is registered, request remote replica subscriptions and full sync.
        // Sending these earlier can race with network responses and drop initial sync before
        // the module subscriptions are discoverable by the runtime.
        let replica_bindings = runtime
            .replica_bindings
            .lock()
            
            .iter()
            .filter(|binding| binding.owner_module_name == module_schema.name)
            .cloned()
            .collect::<Vec<_>>();

        for binding in replica_bindings {
            for event in [TableEvent::Insert, TableEvent::Update, TableEvent::Delete] {
                runtime.network_handle.send_packet(
                    binding.source_node_id,
                    NetworkPacket::RequestSubscription(RequestSubscription {
                        module_name: binding.source_module_name.clone(),
                        table_name: binding.source_table_name.clone(),
                        event,
                    }),
                );
            }

            runtime.network_handle.send_packet(
                binding.source_node_id,
                NetworkPacket::RequestTableSync {
                    module_name: binding.source_module_name.clone(),
                    table_name: binding.source_table_name.clone(),
                },
            );
        }

        setup_file_watches(runtime.clone(), &module_schema)?;

        // Trigger startup events asynchronously via the runtime event queue.
        if fire_init {
            runtime
                .event_sender
                .send((EventInstance::Init {
                    module_name: module_schema.name.clone(),
                }, None))
                .map_err(|err| {
                    IntersticeError::Internal(format!("Failed to send Init event: {}", err))
                })?;
        }
        runtime
            .event_sender
            .send((EventInstance::Load {
                module_name: module_schema.name.clone(),
            }, None))
            .map_err(|err| {
                IntersticeError::Internal(format!("Failed to send Load event: {}", err))
            })?;

        // Logging
        runtime.logger.log(
            &format!("Loaded module '{}'", module_schema.name),
            LogSource::Runtime,
            LogLevel::Info,
        );

        return Ok(module_schema.as_ref().clone());
    }

    pub(crate) async fn request_node_schema(
        &self,
        node_id: crate::node::NodeId,
        node_name: String,
    ) -> Result<NodeSchema, IntersticeError> {
        let request_id = Uuid::new_v4().to_string();
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.pending_schema_responses
            .lock()
            
            .insert(request_id.clone(), sender);

        self.network_handle.send_packet(
            node_id,
            NetworkPacket::SchemaRequest {
                request_id: request_id.clone(),
                node_name,
            },
        );

        match timeout(Duration::from_secs(10), receiver).await {
            Ok(Ok(schema)) => Ok(schema),
            Ok(Err(_)) => Err(IntersticeError::Internal(
                "Schema request channel closed unexpectedly".into(),
            )),
            Err(_) => {
                self.pending_schema_responses
                    .lock()
                    
                    .remove(&request_id);
                Err(IntersticeError::Internal(
                    "Timed out while waiting for remote schema response".into(),
                ))
            }
        }
    }

    pub(crate) fn apply_replica_insert(
        &self,
        source_node_id: crate::node::NodeId,
        source_module_name: &str,
        source_table_name: &str,
        inserted_row: interstice_abi::Row,
    ) {
        let bindings = self
            .replica_bindings
            .lock()
            
            .iter()
            .filter(|binding| {
                binding.source_node_id == source_node_id
                    && binding.source_module_name == source_module_name
                    && binding.source_table_name == source_table_name
            })
            .cloned()
            .collect::<Vec<_>>();

        for binding in bindings {
            let insert_result = self.apply_transaction(
                crate::runtime::transaction::Transaction::Insert {
                    module_name: binding.owner_module_name.clone(),
                    table_name: binding.local_table_name.clone(),
                    new_row: inserted_row.clone(),
                },
                false,
                None,
            );

            if insert_result.is_err() {
                let _ = self.apply_transaction(
                    crate::runtime::transaction::Transaction::Update {
                        module_name: binding.owner_module_name.clone(),
                        table_name: binding.local_table_name.clone(),
                        update_row: inserted_row.clone(),
                    },
                    false,
                    None,
                );
            }

            self.emit_replica_sync_event_if_needed(&binding);
        }
    }

    pub(crate) fn apply_replica_update(
        &self,
        source_node_id: crate::node::NodeId,
        source_module_name: &str,
        source_table_name: &str,
        _old_row: interstice_abi::Row,
        new_row: interstice_abi::Row,
    ) {
        let bindings = self
            .replica_bindings
            .lock()
            
            .iter()
            .filter(|binding| {
                binding.source_node_id == source_node_id
                    && binding.source_module_name == source_module_name
                    && binding.source_table_name == source_table_name
            })
            .cloned()
            .collect::<Vec<_>>();

        for binding in bindings {
            let update_result = self.apply_transaction(
                crate::runtime::transaction::Transaction::Update {
                    module_name: binding.owner_module_name.clone(),
                    table_name: binding.local_table_name.clone(),
                    update_row: new_row.clone(),
                },
                false,
                None,
            );

            if let Err(IntersticeError::RowNotFound { .. }) = update_result {
                let _ = self.apply_transaction(
                    crate::runtime::transaction::Transaction::Insert {
                        module_name: binding.owner_module_name.clone(),
                        table_name: binding.local_table_name.clone(),
                        new_row: new_row.clone(),
                    },
                    false,
                    None,
                );
            }

            self.emit_replica_sync_event_if_needed(&binding);
        }
    }

    pub(crate) fn apply_replica_delete(
        &self,
        source_node_id: crate::node::NodeId,
        source_module_name: &str,
        source_table_name: &str,
        deleted_row: interstice_abi::Row,
    ) {
        let bindings = self
            .replica_bindings
            .lock()
            
            .iter()
            .filter(|binding| {
                binding.source_node_id == source_node_id
                    && binding.source_module_name == source_module_name
                    && binding.source_table_name == source_table_name
            })
            .cloned()
            .collect::<Vec<_>>();

        for binding in bindings {
            if let Ok(primary_key) = TryInto::<IndexKey>::try_into(deleted_row.primary_key.clone())
            {
                let _ = self.apply_transaction(
                    crate::runtime::transaction::Transaction::Delete {
                        module_name: binding.owner_module_name.clone(),
                        table_name: binding.local_table_name.clone(),
                        deleted_row_id: primary_key,
                    },
                    false,
                    None,
                );
            }

            self.emit_replica_sync_event_if_needed(&binding);
        }
    }

    pub(crate) fn apply_replica_full_sync(
        &self,
        source_node_id: crate::node::NodeId,
        source_module_name: &str,
        source_table_name: &str,
        rows: Vec<interstice_abi::Row>,
    ) {
        let bindings = self
            .replica_bindings
            .lock()
            
            .iter()
            .filter(|binding| {
                binding.source_node_id == source_node_id
                    && binding.source_module_name == source_module_name
                    && binding.source_table_name == source_table_name
            })
            .cloned()
            .collect::<Vec<_>>();

        for binding in bindings {
            let mut restored = false;
            if let Some(module) = self.modules.lock().get(&binding.owner_module_name) {
                if let Some(table) = module
                    .tables
                    .lock()
                    
                    .get_mut(&binding.local_table_name)
                {
                    let _ = table.restore_from_rows(rows.clone());
                    restored = true;
                }
            }

            if restored {
                self.emit_replica_sync_event_if_needed(&binding);
            }
        }
    }

    fn emit_replica_sync_event_if_needed(&self, binding: &crate::runtime::ReplicaBinding) {
        let key = format!(
            "{}|{}|{}|{}",
            binding.owner_module_name,
            binding.source_node_name,
            binding.source_module_name,
            binding.source_table_name
        );

        let should_emit = self.emitted_replica_sync_events.lock().insert(key);

        if should_emit {
            let _ = self.event_sender.send((EventInstance::ReplicaTableSynced {
                node_name: binding.source_node_name.clone(),
                module_name: binding.source_module_name.clone(),
                table_name: binding.source_table_name.clone(),
            }, None));
        }
    }

    pub fn remove_module(runtime: Arc<Runtime>, module_name: &str) {
        let module_schema = runtime
            .modules
            .lock()
            
            .get(module_name)
            .map(|module| module.schema.as_ref().clone());

        if let Some(schema) = &module_schema {
            for sub in &schema.subscriptions {
                match &sub.event {
                    SubscriptionEventSchema::Insert {
                        node_selection: NodeSelection::Other(node_name),
                        module_name: source_module_name,
                        table_name: source_table_name,
                    }
                    | SubscriptionEventSchema::Update {
                        node_selection: NodeSelection::Other(node_name),
                        module_name: source_module_name,
                        table_name: source_table_name,
                    }
                    | SubscriptionEventSchema::Delete {
                        node_selection: NodeSelection::Other(node_name),
                        module_name: source_module_name,
                        table_name: source_table_name,
                    } => {
                        if let Some(node_dependency) = schema
                            .node_dependencies
                            .iter()
                            .find(|dep| dep.name == *node_name)
                        {
                            if let Ok(node_id) = runtime
                                .network_handle
                                .get_node_id_from_adress(&node_dependency.address)
                            {
                                let event = match &sub.event {
                                    SubscriptionEventSchema::Insert { .. } => TableEvent::Insert,
                                    SubscriptionEventSchema::Update { .. } => TableEvent::Update,
                                    SubscriptionEventSchema::Delete { .. } => TableEvent::Delete,
                                    _ => unreachable!(),
                                };

                                runtime.network_handle.send_packet(
                                    node_id,
                                    NetworkPacket::RequestUnsubscription(RequestSubscription {
                                        module_name: source_module_name.clone(),
                                        table_name: source_table_name.clone(),
                                        event,
                                    }),
                                );
                            }
                        }
                    }
                    _ => {}
                }
            }

            for replicated in &schema.replicated_tables {
                if let Some(node_dependency) = schema
                    .node_dependencies
                    .iter()
                    .find(|dep| dep.name == replicated.node_name)
                {
                    if let Ok(node_id) = runtime
                        .network_handle
                        .get_node_id_from_adress(&node_dependency.address)
                    {
                        for event in [TableEvent::Insert, TableEvent::Update, TableEvent::Delete] {
                            runtime.network_handle.send_packet(
                                node_id,
                                NetworkPacket::RequestUnsubscription(RequestSubscription {
                                    module_name: replicated.module_name.clone(),
                                    table_name: replicated.table_name.clone(),
                                    event,
                                }),
                            );
                        }
                    }
                }
            }
        }

        // Subtract table subscriptions from active_subscription_count
        if let Some(schema) = &module_schema {
            let table_sub_count = schema
                .subscriptions
                .iter()
                .filter(|s| matches!(
                    s.event,
                    SubscriptionEventSchema::Insert { .. }
                        | SubscriptionEventSchema::Update { .. }
                        | SubscriptionEventSchema::Delete { .. }
                ))
                .count() as i32;
            if table_sub_count > 0 {
                runtime
                    .active_subscription_count
                    .fetch_sub(table_sub_count, std::sync::atomic::Ordering::Relaxed);
            }
        }

        runtime.modules.lock().remove(module_name);
        runtime.clear_reducer_access_cache();
        runtime
            .replica_bindings
            .lock()
            
            .retain(|binding| binding.owner_module_name != module_name);
        runtime
            .emitted_replica_sync_events
            .lock()
            
            .retain(|key| !key.starts_with(&format!("{}|", module_name)));
        if let Some(modules_path) = &runtime.modules_path {
            let module_dir = modules_path.join(module_name);
            if module_dir.exists() {
                std::fs::remove_dir_all(module_dir).unwrap();
            }
        }

        // Removing module from authority modules if it has any authority
        let authorities_to_remove = runtime
            .authority_modules
            .lock()
            
            .iter()
            .filter(|(_, entry)| entry.module_name() == module_name)
            .map(|(authority, _)| authority.clone())
            .collect::<Vec<_>>();
        for authority in authorities_to_remove {
            runtime.authority_modules.lock().remove(&authority);
        }

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
                                let _ = event_sender.send((EventInstance::File(ev), None));
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
            
            .extend(watchers_for_module);
    }

    Ok(())
}
