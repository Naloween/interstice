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
    NodeSelection, RawQueryContext as QueryContext,
    SubscriptionEventSchema, TableVisibility,
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
    /// Each idle instance is paired with the reducer it last ran (see
    /// [`Module::call_reducer`] for the affinity rule).
    instance_pool: Arc<(parking_lot::Mutex<Vec<(WasmInstance, Option<String>)>>, parking_lot::Condvar)>,
    query_instance: Arc<Mutex<WasmInstance>>,
    /// Builds a fresh, fully initialised instance. Used to replace an
    /// instance after a trap: its linear memory may be mid-update, so it is
    /// never reused.
    factory: InstanceFactory,
    /// See [`interstice_abi::RawReducerContext::state_epoch`].
    state_epoch: std::sync::atomic::AtomicU64,
    wasm_bytes: Vec<u8>,
    pub schema: Arc<ModuleSchema>,
    pub tables: Arc<parking_lot::RwLock<HashMap<String, Table>>>,
    pub reducer_names: HashSet<String>,
    /// Record log (DESIGN §6.7); `recording` is its generation, 0 when off.
    pub(crate) recorder: Mutex<Option<crate::runtime::record::Recorder>>,
    pub(crate) recording: std::sync::atomic::AtomicU64,
}

/// Instantiates `wasm_module` in a fresh store. `schema` is `None` while the
/// schema is still unknown (first instance, used to load it).
struct InstanceFactory {
    runtime: Arc<Runtime>,
    wasm_module: wasmtimeModule,
}

impl InstanceFactory {
    fn instantiate(&self, schema: Option<&Arc<ModuleSchema>>) -> Result<WasmInstance, IntersticeError> {
        let mut store = Store::new(&self.runtime.engine, StoreState::new(self.runtime.clone()));
        let raw = self
            .runtime
            .linker
            .instantiate(&mut store, &self.wasm_module)
            .map_err(|e| IntersticeError::Internal(format!("wasm instantiation failed: {e}")))?;
        let mut inst = WasmInstance::new(store, raw)?;
        if let Some(schema) = schema {
            let funcs: Vec<String> = schema
                .reducers
                .iter()
                .map(|r| get_reducer_wrapper_name(&r.name))
                .chain(schema.queries.iter().map(|q| get_query_wrapper_name(&q.name)))
                .collect();
            inst.store.data_mut().module_schema = schema.clone();
            inst.preload_funcs(&funcs);
            inst.init_scratch(4096).ok();
        }
        Ok(inst)
    }
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
        let wasm_bytes = std::fs::read(path).map_err(|e| {
            IntersticeError::Internal(format!("cannot read module {}: {e}", path.display()))
        })?;
        Self::from_bytes(runtime, &wasm_bytes).await
    }

    pub async fn from_bytes(
        runtime: Arc<Runtime>,
        wasm_binary: &[u8],
    ) -> Result<Self, IntersticeError> {
        Self::build(runtime, wasm_binary)
    }

    /// Compile and instantiate a module without registering it.
    pub(crate) fn build(runtime: Arc<Runtime>, wasm_binary: &[u8]) -> Result<Self, IntersticeError> {
        let wasm_module = wasmtimeModule::new(&runtime.engine, wasm_binary)
            .map_err(|e| IntersticeError::Internal(format!("invalid wasm module: {e}")))?;
        let factory = InstanceFactory {
            runtime,
            wasm_module,
        };

        let schema = Arc::new(factory.instantiate(None)?.load_schema()?);
        if schema.abi_version != ABI_VERSION {
            return Err(IntersticeError::AbiVersionMismatch {
                expected: ABI_VERSION,
                found: schema.abi_version,
            });
        }

        let mut instances = Vec::with_capacity(REDUCER_POOL_SIZE);
        for _ in 0..REDUCER_POOL_SIZE {
            instances.push((factory.instantiate(Some(&schema))?, None));
        }
        let query_instance = factory.instantiate(Some(&schema))?;

        let tables = schema
            .tables
            .iter()
            .map(|table_schema| (table_schema.name.clone(), Table::new(table_schema.clone())))
            .collect();
        let reducer_names = schema.reducers.iter().map(|r| r.name.clone()).collect();

        Ok(Self {
            instance_pool: Arc::new((
                parking_lot::Mutex::new(instances),
                parking_lot::Condvar::new(),
            )),
            query_instance: Arc::new(Mutex::new(query_instance)),
            factory,
            state_epoch: std::sync::atomic::AtomicU64::new(0),
            wasm_bytes: wasm_binary.to_vec(),
            schema,
            tables: Arc::new(parking_lot::RwLock::new(tables)),
            reducer_names,
            recorder: Mutex::new(None),
            recording: std::sync::atomic::AtomicU64::new(0),
        })
    }

    pub fn wasm_bytes(&self) -> &[u8] {
        &self.wasm_bytes
    }

    pub(crate) fn runtime(&self) -> &Arc<Runtime> {
        &self.factory.runtime
    }

    /// Current [`interstice_abi::RawReducerContext::state_epoch`].
    pub fn state_epoch(&self) -> u64 {
        self.state_epoch.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Signal that effects of a call were dropped after the instance had
    /// already observed them (rejected commit): instance caches must reset.
    pub fn bump_state_epoch(&self) {
        self.state_epoch
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    }

    /// Replace `instance` with a fresh one if the call trapped.
    fn recycle<T>(&self, instance: &mut WasmInstance, result: &Result<T, IntersticeError>) {
        if matches!(result, Err(IntersticeError::WasmTrap(_))) {
            match self.factory.instantiate(Some(&self.schema)) {
                Ok(fresh) => *instance = fresh,
                Err(err) => self.factory.runtime.logger.log(
                    &format!(
                        "Module '{}': could not replace trapped instance: {err}",
                        self.schema.name
                    ),
                    LogSource::Runtime,
                    LogLevel::Error,
                ),
            }
        }
    }

    /// Run `reducer` with its input already encoded as `(ReducerContext, args)`.
    pub fn call_reducer_encoded(&self, reducer: &str, input: &[u8]) -> Result<(), IntersticeError> {
        let func_name = get_reducer_wrapper_name(reducer);
        // Borrow an instance from the pool; block if all are in use.
        //
        // Affinity: prefer the idle instance that last ran this same reducer,
        // else the least recently returned one, so other reducers' warm
        // instances are left alone. Instances may hold
        // instance caches (`interstice_sdk::InstanceCache`) that a reducer
        // builds for itself; keeping a reducer on the same instance keeps
        // those warm. This is purely a performance property: any instance can
        // serve any call, and caches are rebuilt from tables when missing.
        let (lock, cvar) = &*self.instance_pool;
        let mut instance = {
            let mut pool = lock.lock();
            while pool.is_empty() {
                cvar.wait(&mut pool);
            }
            let idx = pool
                .iter()
                .rposition(|(_, last)| last.as_deref() == Some(reducer))
                .unwrap_or(0);
            pool.remove(idx).0
        };
        let result = instance.call_reducer(&func_name, input);
        self.recycle(&mut instance, &result);
        // Return the instance to the pool and wake a waiting thread if any.
        {
            let mut pool = lock.lock();
            pool.push((instance, Some(reducer.to_string())));
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
        let mut instance = self.query_instance.lock();
        let result = instance.call_query(&func_name, args);
        self.recycle(&mut instance, &result);
        result
    }
}

impl Runtime {
    pub async fn load_module(
        runtime: Arc<Self>,
        module: Module,
    ) -> Result<ModuleSchema, IntersticeError> {
        let module_schema = module.schema.clone();

        // Decide first-load vs reload from persistent state, independently of how
        // the module was loaded (CLI, network, module manager, or node startup).
        // A module has been loaded here before iff its `state.toml` exists (we
        // write it on every load and only delete it on `remove`). On a reload we
        // restore its tables from disk and skip the once-only `init` event,
        // exactly like a node restart; on a genuine first load we fire `init`.
        let is_reload = runtime
            .modules_path
            .as_ref()
            .map(|path| path.join(&module_schema.name).join("state.toml").exists())
            .unwrap_or(false);

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
                    Authority::Network => AuthorityEntry::Network {
                        module_name,
                        on_event_reducer: module_schema
                            .subscriptions
                            .iter()
                            .find(|sub| sub.event == SubscriptionEventSchema::Network)
                            .map(|sub| sub.reducer_name.clone()),
                    },
                };
                runtime
                    .authority_modules
                    .lock()
                    
                    .insert(authority.clone(), entry);
            }
        }

        module
            .schema
            .check_no_blob_in_tables()
            .map_err(IntersticeError::Internal)?;

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
                .write()
                
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
            // Mark the module as loaded so it is auto-loaded again on node startup.
            write_module_state(&module_dir, true);
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

        // On a reload, repopulate the module's tables from their persisted data.
        if is_reload {
            if let Some(module) = runtime.modules.lock().get(&module_schema.name) {
                let mut tables = module.tables.write();
                for table in tables.values_mut() {
                    runtime
                        .persistence
                        .restore_table(&module_schema.name, table)?;
                }
            }
        }

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
        // `init` fires only on the first load; a reload does not re-run it.
        if !is_reload {
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

    /// Run a [`crate::runtime::reducer::Job::Replica`]: apply a remote change
    /// to every local replica of that table.
    pub(crate) fn apply_replica_change(&self, change: crate::runtime::reducer::ReplicaChange) {
        use crate::runtime::reducer::ReplicaChange;
        match change {
            ReplicaChange::Insert { source_node_id, module_name, table_name, row } => {
                self.apply_replica_insert(source_node_id, &module_name, &table_name, row)
            }
            ReplicaChange::Update { source_node_id, module_name, table_name, old_row, new_row } => {
                self.apply_replica_update(
                    source_node_id,
                    &module_name,
                    &table_name,
                    old_row,
                    new_row,
                )
            }
            ReplicaChange::Delete { source_node_id, module_name, table_name, row } => {
                self.apply_replica_delete(source_node_id, &module_name, &table_name, row)
            }
            ReplicaChange::FullSync { source_node_id, module_name, table_name, rows } => {
                self.apply_replica_full_sync(source_node_id, &module_name, &table_name, rows)
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
                    .write()
                    
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

    /// Tear down a module's live runtime presence: unsubscribe from remote
    /// tables, drop its subscription counts, remove it from the modules map,
    /// clear caches and replica bindings, and release any authorities it held.
    /// Does NOT touch the module's persisted data on disk — shared by both
    /// `unload_module` (keeps data) and `remove_module` (deletes data after).
    fn teardown_module(runtime: &Arc<Runtime>, module_name: &str) {
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
    }

    /// Unload a module from the runtime while keeping its persisted data on
    /// disk, so it can be loaded again later and resume with its tables intact.
    /// Ephemeral tables are wiped (by design — same as a node restart). The
    /// module is marked unloaded in its `state.toml` so node startup skips it.
    pub fn unload_module(runtime: Arc<Runtime>, module_name: &str) {
        Self::teardown_module(&runtime, module_name);

        if let Some(modules_path) = &runtime.modules_path {
            let module_dir = modules_path.join(module_name);
            if module_dir.exists() {
                write_module_state(&module_dir, false);
            }
        }

        runtime.logger.log(
            &format!("Unloaded module '{}'", module_name),
            LogSource::Runtime,
            LogLevel::Info,
        );
    }

    /// Remove a module and delete all of its persisted data (full uninstall).
    pub fn remove_module(runtime: Arc<Runtime>, module_name: &str) {
        Self::teardown_module(&runtime, module_name);

        if let Some(modules_path) = &runtime.modules_path {
            let module_dir = modules_path.join(module_name);
            if module_dir.exists() {
                std::fs::remove_dir_all(module_dir).unwrap();
            }
        }

        runtime.logger.log(
            &format!("Removed module '{}'", module_name),
            LogSource::Runtime,
            LogLevel::Info,
        );
    }
}

/// Persisted per-module state stored as `state.toml` in the module's data dir.
#[derive(serde::Serialize, serde::Deserialize)]
struct ModuleState {
    /// Whether the module should be auto-loaded when the node starts.
    loaded: bool,
}

/// Write `state.toml` into the given module directory.
fn write_module_state(module_dir: &Path, loaded: bool) {
    if let Ok(contents) = toml::to_string(&ModuleState { loaded }) {
        let _ = std::fs::write(module_dir.join("state.toml"), contents);
    }
}

/// Whether the module in `module_dir` should be auto-loaded on node startup.
/// Defaults to `true` when no `state.toml` is present (backward compatible).
pub(crate) fn module_should_load(module_dir: &Path) -> bool {
    match std::fs::read_to_string(module_dir.join("state.toml")) {
        Ok(contents) => toml::from_str::<ModuleState>(&contents)
            .map(|state| state.loaded)
            .unwrap_or(true),
        Err(_) => true,
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
