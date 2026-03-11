mod authority;
mod deterministic_random;
pub mod event;
pub mod host_calls;
pub mod module;
mod query;
pub mod reducer;
pub mod table;
pub mod transaction;
mod wasm;

pub(crate) use authority::AuthorityEntry;

use crate::{
    IntersticeError,
    logger::{LogLevel, LogSource, Logger},
    network::NetworkHandle,
    node::NodeId,
    persistence::TableStore,
    runtime::{
        event::EventInstance,
        host_calls::{
            audio::AudioState,
            gpu::{GpuCallRequest, GpuState},
        },
        module::Module,
        reducer::{CallFrame, CompletionGuard, ReducerJob},
        wasm::{StoreState, linker::define_host_calls},
    },
};
use interstice_abi::{
    Authority, IntersticeValue, ModuleEvent, NodeSchema, SubscriptionEventSchema, TableVisibility,
};
use notify::RecommendedWatcher;
use std::sync::atomic::AtomicU64;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
};
use tokio::sync::{
    Notify,
    mpsc::{UnboundedReceiver, UnboundedSender},
    oneshot,
};
use wasmtime::{Config, Engine, Linker};

pub struct Runtime {
    pub(crate) node_id: NodeId,
    pub(crate) gpu: Arc<Mutex<Option<GpuState>>>,
    pub(crate) audio_state: Arc<Mutex<AudioState>>,
    pub(crate) modules: Arc<Mutex<HashMap<String, Arc<Module>>>>,
    pub(crate) authority_modules: Arc<Mutex<HashMap<Authority, AuthorityEntry>>>,
    pub(crate) call_stack: Arc<Mutex<Vec<CallFrame>>>,
    pub(crate) engine: Arc<Engine>,
    pub(crate) linker: Arc<Linker<StoreState>>,
    pub(crate) event_sender: UnboundedSender<EventInstance>,
    pub(crate) network_handle: NetworkHandle,
    pub(crate) persistence: Arc<TableStore>,
    pub(crate) logger: Logger,
    run_app_notify: Arc<Notify>,
    pub(crate) app_initialized: Arc<Mutex<bool>>,
    pub(crate) pending_query_responses:
        Arc<Mutex<HashMap<String, oneshot::Sender<IntersticeValue>>>>,
    pub(crate) pending_schema_responses: Arc<Mutex<HashMap<String, oneshot::Sender<NodeSchema>>>>,
    pub(crate) reducer_sender: UnboundedSender<ReducerJob>,
    reducer_receiver: Mutex<Option<UnboundedReceiver<ReducerJob>>>,
    pub(crate) gpu_call_sender: mpsc::Sender<GpuCallRequest>,
    gpu_call_receiver: Mutex<Option<mpsc::Receiver<GpuCallRequest>>>,
    pub(crate) modules_path: Option<PathBuf>,
    node_subscriptions: Arc<Mutex<HashMap<NodeId, Vec<SubscriptionEventSchema>>>>,
    pub(crate) node_names_by_id: Arc<Mutex<HashMap<NodeId, String>>>,
    pub(crate) replica_bindings: Arc<Mutex<Vec<ReplicaBinding>>>,
    pub(crate) emitted_replica_sync_events: Arc<Mutex<HashSet<String>>>,
    pub(crate) file_watchers: Arc<Mutex<Vec<RecommendedWatcher>>>,
    pub(crate) call_sequence: AtomicU64,
}

#[derive(Debug, Clone)]
pub(crate) struct ReplicaBinding {
    pub owner_module_name: String,
    pub source_node_id: NodeId,
    pub source_node_name: String,
    pub source_module_name: String,
    pub source_table_name: String,
    pub local_table_name: String,
}

impl Runtime {
    pub fn new(
        node_id: NodeId,
        modules_path: Option<PathBuf>,
        table_store: TableStore,
        event_sender: UnboundedSender<EventInstance>,
        network_handle: NetworkHandle,
        audio_state: Arc<Mutex<AudioState>>,
        gpu: Arc<Mutex<Option<GpuState>>>,
        run_app_notify: Arc<Notify>,
        logger: Logger,
    ) -> Result<Self, IntersticeError> {
        let (reducer_sender, reducer_receiver) =
            tokio::sync::mpsc::unbounded_channel::<ReducerJob>();
        let (gpu_call_sender, gpu_call_receiver) = mpsc::channel::<GpuCallRequest>();
        let mut config = Config::new();
        config.async_support(true);
        let engine = Arc::new(Engine::new(&config).unwrap());
        let mut linker = Linker::new(&engine);
        define_host_calls(&mut linker).map_err(|err| {
            IntersticeError::Internal(format!("Couldn't add host calls to the linker: {}", err))
        })?;
        Ok(Self {
            node_id,
            gpu,
            audio_state,
            modules: Arc::new(Mutex::new(HashMap::new())),
            authority_modules: Arc::new(Mutex::new(HashMap::new())),
            call_stack: Arc::new(Mutex::new(Vec::new())),
            engine,
            linker: Arc::new(linker),
            event_sender,
            network_handle,
            persistence: Arc::new(table_store),
            app_initialized: Arc::new(Mutex::new(false)),
            pending_query_responses: Arc::new(Mutex::new(HashMap::new())),
            pending_schema_responses: Arc::new(Mutex::new(HashMap::new())),
            reducer_sender,
            reducer_receiver: Mutex::new(Some(reducer_receiver)),
            gpu_call_sender,
            gpu_call_receiver: Mutex::new(Some(gpu_call_receiver)),
            modules_path,
            run_app_notify,
            node_subscriptions: Arc::new(Mutex::new(HashMap::new())),
            node_names_by_id: Arc::new(Mutex::new(HashMap::new())),
            replica_bindings: Arc::new(Mutex::new(Vec::new())),
            emitted_replica_sync_events: Arc::new(Mutex::new(HashSet::new())),
            logger,
            file_watchers: Arc::new(Mutex::new(Vec::new())),
            call_sequence: AtomicU64::new(0),
        })
    }

    pub fn take_gpu_call_receiver(&self) -> mpsc::Receiver<GpuCallRequest> {
        self.gpu_call_receiver
            .lock()
            .unwrap()
            .take()
            .expect("Gpu call receiver already taken")
    }

    pub async fn run(runtime: Arc<Runtime>, mut event_receiver: UnboundedReceiver<EventInstance>) {
        let mut reducer_receiver = runtime
            .reducer_receiver
            .lock()
            .unwrap()
            .take()
            .expect("Reducer receiver already taken");
        let reducer_runtime = runtime.clone();
        tokio::task::spawn_local(async move {
            while let Some(job) = reducer_receiver.recv().await {
                let _completion_guard = job.completion.map(CompletionGuard::new);

                let _ = reducer_runtime
                    .call_reducer(
                        &job.module_name,
                        &job.reducer_name,
                        job.input,
                        job.caller_node_id,
                    )
                    .await;
            }
        });

        while let Some(event) = event_receiver.recv().await {
            Runtime::handle_event(runtime.clone(), event, &mut event_receiver).await;
        }
    }

    async fn handle_event(
        runtime: Arc<Runtime>,
        event: EventInstance,
        _event_receiver: &mut UnboundedReceiver<EventInstance>,
    ) {
        match event {
            EventInstance::RequestAppInitialization => {
                let app_initialized = runtime.app_initialized.lock().unwrap();
                if *app_initialized {
                    runtime.logger.log(
                        "Received duplicate app initialization request",
                        LogSource::Runtime,
                        LogLevel::Warning,
                    );
                } else {
                    runtime.logger.log(
                        "Received app initialization request",
                        LogSource::Runtime,
                        LogLevel::Info,
                    );
                    runtime.run_app_notify.notify_one();
                }
            }
            EventInstance::AppInitialized => {
                let mut app_initialized = runtime.app_initialized.lock().unwrap();
                if *app_initialized {
                    runtime.logger.log(
                        "Received duplicate App initialized event",
                        LogSource::Runtime,
                        LogLevel::Warning,
                    );
                } else {
                    *app_initialized = true;
                    runtime
                        .logger
                        .log("App initialized", LogSource::Runtime, LogLevel::Info);
                }
            }
            EventInstance::RemoteReducerCall {
                module_name,
                reducer_name,
                input,
                requesting_node_id,
            } => {
                let _ = runtime.reducer_sender.send(ReducerJob {
                    module_name,
                    reducer_name,
                    input,
                    caller_node_id: requesting_node_id,
                    completion: None,
                });
            }
            EventInstance::RemoteQueryCall {
                requesting_node_id,
                request_id,
                module_name,
                query_name,
                input,
            } => {
                let runtime = runtime.clone();
                tokio::task::spawn_local(async move {
                    let result = match runtime
                        .call_query(&module_name, &query_name, input, requesting_node_id)
                        .await
                    {
                        Ok(value) => value,
                        Err(err) => {
                            runtime.logger.log(
                                &format!(
                                    "Remote query '{}' on module '{}' failed: {}",
                                    query_name, module_name, err
                                ),
                                LogSource::Runtime,
                                LogLevel::Error,
                            );
                            IntersticeValue::Void
                        }
                    };
                    runtime.network_handle.send_packet(
                        requesting_node_id,
                        crate::network::protocol::NetworkPacket::QueryResponse {
                            request_id,
                            result,
                        },
                    );
                });
            }
            EventInstance::RemoteQueryResponse { request_id, result } => {
                if let Some(sender) = runtime
                    .pending_query_responses
                    .lock()
                    .unwrap()
                    .remove(&request_id)
                {
                    let _ = sender.send(result);
                }
            }
            EventInstance::RemoteSchemaResponse { request_id, schema } => {
                if let Some(sender) = runtime
                    .pending_schema_responses
                    .lock()
                    .unwrap()
                    .remove(&request_id)
                {
                    let _ = sender.send(schema);
                }
            }
            EventInstance::RequestSubscription {
                requesting_node_id,
                event,
            } => {
                let mut subscriptions_by_node = runtime.node_subscriptions.lock().unwrap();
                let subscriptions = subscriptions_by_node
                    .entry(requesting_node_id)
                    .or_insert(Vec::new());
                if !subscriptions.contains(&event) {
                    subscriptions.push(event);
                }
            }
            EventInstance::RequestUnsubscription {
                requesting_node_id,
                event,
            } => {
                let mut subscriptions_by_node = runtime.node_subscriptions.lock().unwrap();
                if let Some(subscriptions) = subscriptions_by_node.get_mut(&requesting_node_id) {
                    subscriptions.retain(|existing| existing != &event);
                    if subscriptions.is_empty() {
                        subscriptions_by_node.remove(&requesting_node_id);
                    }
                }
            }
            EventInstance::RequestTableSync {
                requesting_node_id,
                module_name,
                table_name,
            } => {
                let rows_result = {
                    let modules = runtime.modules.lock().unwrap();
                    let module = modules.get(&module_name).ok_or_else(|| {
                        IntersticeError::ModuleNotFound(
                            module_name.clone(),
                            "When handling table sync request".to_string(),
                        )
                    });

                    module.and_then(|module| {
                        let table_schema = module
                            .schema
                            .tables
                            .iter()
                            .find(|table| table.name == table_name)
                            .ok_or_else(|| IntersticeError::TableNotFound {
                                module_name: module_name.clone(),
                                table_name: table_name.clone(),
                            })?;

                        if table_schema.visibility != TableVisibility::Public {
                            return Err(IntersticeError::Internal(format!(
                                "Table sync denied for non-public table '{}.{}'",
                                module_name, table_name
                            )));
                        }

                        let tables = module.tables.lock().unwrap();
                        let table = tables.get(&table_name).ok_or_else(|| {
                            IntersticeError::TableNotFound {
                                module_name: module_name.clone(),
                                table_name: table_name.clone(),
                            }
                        })?;

                        Ok(table.scan().to_vec())
                    })
                };

                match rows_result {
                    Ok(rows) => runtime.network_handle.send_packet(
                        requesting_node_id,
                        crate::network::protocol::NetworkPacket::TableSyncResponse {
                            module_name,
                            table_name,
                            rows,
                        },
                    ),
                    Err(err) => runtime.network_handle.send_packet(
                        requesting_node_id,
                        crate::network::protocol::NetworkPacket::Error(err.to_string()),
                    ),
                }
            }
            EventInstance::RemoteTableSync {
                source_node_id,
                module_name,
                table_name,
                rows,
            } => {
                runtime.apply_replica_full_sync(source_node_id, &module_name, &table_name, rows);
            }
            EventInstance::PublishModule {
                wasm_binary,
                source_node_id,
            } => {
                if runtime
                    .authority_modules
                    .lock()
                    .unwrap()
                    .contains_key(&Authority::Module)
                {
                    let module_name = match Module::from_bytes(runtime.clone(), &wasm_binary).await
                    {
                        Ok(module) => module.schema.name.clone(),
                        Err(err) => {
                            runtime.logger.log(
                                &format!(
                                    "Failed to decode published module before forwarding to module authority: {}",
                                    err
                                ),
                                LogSource::Runtime,
                                LogLevel::Error,
                            );
                            return;
                        }
                    };

                    let _ = runtime.event_sender.send(EventInstance::Module(
                        ModuleEvent::PublishRequest {
                            node_id: source_node_id.to_string(),
                            module_name,
                            wasm_binary,
                        },
                    ));
                } else {
                    let runtime_cloned = runtime.clone();
                    tokio::task::spawn_local(async move {
                        let module = match Module::from_bytes(runtime_cloned.clone(), &wasm_binary)
                            .await
                        {
                            Ok(module) => module,
                            Err(err) => {
                                runtime_cloned.logger.log(
                                    &format!("Failed to decode published module bytes: {}", err),
                                    LogSource::Runtime,
                                    LogLevel::Error,
                                );
                                return;
                            }
                        };

                        let module_name = module.schema.name.clone();
                        if runtime_cloned
                            .modules
                            .lock()
                            .unwrap()
                            .contains_key(&module_name)
                        {
                            Runtime::remove_module(runtime_cloned.clone(), &module_name);
                        }

                        if let Err(err) =
                            Runtime::load_module(runtime_cloned.clone(), module, true).await
                        {
                            runtime_cloned.logger.log(
                                &format!(
                                    "Failed to load published module '{}': {}",
                                    module_name, err
                                ),
                                LogSource::Runtime,
                                LogLevel::Error,
                            );
                        }
                    });
                }
            }
            EventInstance::RemoveModule {
                module_name,
                source_node_id,
            } => {
                if runtime
                    .authority_modules
                    .lock()
                    .unwrap()
                    .contains_key(&Authority::Module)
                {
                    let _ = runtime.event_sender.send(EventInstance::Module(
                        ModuleEvent::RemoveRequest {
                            node_id: source_node_id.to_string(),
                            module_name,
                        },
                    ));
                } else {
                    Runtime::remove_module(runtime.clone(), &module_name);
                }
            }
            EventInstance::SchemaRequest {
                requesting_node_id,
                request_id,
                node_name,
            } => {
                let schema = runtime.build_node_schema(node_name);
                runtime.network_handle.send_packet(
                    requesting_node_id,
                    crate::network::protocol::NetworkPacket::SchemaResponse { request_id, schema },
                );
            }
            EventInstance::TableInsertEvent {
                source_node_id,
                module_name,
                table_name,
                inserted_row,
            } => {
                if let Some(source_node_id) = source_node_id {
                    runtime.apply_replica_insert(
                        source_node_id,
                        &module_name,
                        &table_name,
                        inserted_row.clone(),
                    );
                }

                let event = EventInstance::TableInsertEvent {
                    source_node_id,
                    module_name,
                    table_name,
                    inserted_row,
                };
                let triggered = runtime.find_subscriptions(&event).unwrap();
                for sub in triggered {
                    let _ = runtime.invoke_subscription(sub, event.clone());
                }
            }
            EventInstance::TableUpdateEvent {
                source_node_id,
                module_name,
                table_name,
                old_row,
                new_row,
            } => {
                if let Some(source_node_id) = source_node_id {
                    runtime.apply_replica_update(
                        source_node_id,
                        &module_name,
                        &table_name,
                        old_row.clone(),
                        new_row.clone(),
                    );
                }

                let event = EventInstance::TableUpdateEvent {
                    source_node_id,
                    module_name,
                    table_name,
                    old_row,
                    new_row,
                };
                let triggered = runtime.find_subscriptions(&event).unwrap();
                for sub in triggered {
                    let _ = runtime.invoke_subscription(sub, event.clone());
                }
            }
            EventInstance::TableDeleteEvent {
                source_node_id,
                module_name,
                table_name,
                deleted_row,
            } => {
                if let Some(source_node_id) = source_node_id {
                    runtime.apply_replica_delete(
                        source_node_id,
                        &module_name,
                        &table_name,
                        deleted_row.clone(),
                    );
                }

                let event = EventInstance::TableDeleteEvent {
                    source_node_id,
                    module_name,
                    table_name,
                    deleted_row,
                };
                let triggered = runtime.find_subscriptions(&event).unwrap();
                for sub in triggered {
                    let _ = runtime.invoke_subscription(sub, event.clone());
                }
            }
            event => {
                let triggered = runtime.find_subscriptions(&event).unwrap();

                for sub in triggered {
                    let _ = runtime.invoke_subscription(sub, event.clone());
                }
            }
        }
    }

    fn build_node_schema(&self, name: String) -> NodeSchema {
        NodeSchema {
            name,
            address: self.network_handle.address.clone(),
            modules: self
                .modules
                .lock()
                .unwrap()
                .values()
                .map(|m| (*m.schema).clone())
                .collect(),
        }
    }

    pub fn replay(&self) -> Result<(), IntersticeError> {
        let module_entries = self
            .modules
            .lock()
            .unwrap()
            .iter()
            .map(|(name, module)| (name.clone(), Arc::clone(module)))
            .collect::<Vec<_>>();

        for (module_name, module) in module_entries {
            let mut tables = module.tables.lock().unwrap();
            for table in tables.values_mut() {
                self.persistence.restore_table(&module_name, table)?;
            }
        }

        Ok(())
    }
}
