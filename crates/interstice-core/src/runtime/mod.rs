mod authority;
mod deterministic_random;
pub mod event;
pub mod host_calls;
pub mod module;
mod query;
pub mod reducer;
mod scheduler;
mod table_access;
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
        reducer::{ACTIVE_COMPLETION, CompletionToken, ReducerJob},
        wasm::{StoreState, linker::define_host_calls},
    },
};
use interstice_abi::{
    Authority, IntersticeValue, ModuleEvent, NodeSchema, SubscriptionEventSchema, TableVisibility,
};
use notify::RecommendedWatcher;
use std::sync::atomic::{AtomicI32, AtomicU64};
use parking_lot::Mutex;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, mpsc},
};
use tokio::sync::{
    Notify,
    mpsc::{UnboundedReceiver, UnboundedSender},
    oneshot,
};
use crossbeam_channel::{Receiver as CbReceiver, Sender as CbSender};
use std::sync::mpsc as std_mpsc;
use wasmtime::{Config, Engine, Linker};

pub struct Runtime {
    pub(crate) node_id: NodeId,
    pub(crate) gpu: Arc<Mutex<Option<GpuState>>>,
    pub(crate) audio_state: Arc<Mutex<AudioState>>,
    pub(crate) modules: Arc<Mutex<HashMap<String, Arc<Module>>>>,
    pub(crate) authority_modules: Arc<Mutex<HashMap<Authority, AuthorityEntry>>>,
    pub(crate) engine: Arc<Engine>,
    pub(crate) linker: Arc<Linker<StoreState>>,
    pub(crate) event_sender: UnboundedSender<(EventInstance, Option<CompletionToken>)>,
    pub(crate) network_handle: NetworkHandle,
    pub(crate) persistence: Arc<TableStore>,
    pub(crate) logger: Logger,
    run_app_notify: Arc<Notify>,
    pub(crate) app_initialized: Arc<Mutex<bool>>,
    /// Sync mpsc channels — lets the dedicated WASM thread do a blocking recv for remote queries.
    pub(crate) pending_query_responses:
        Arc<Mutex<HashMap<String, std_mpsc::Sender<IntersticeValue>>>>,
    pub(crate) pending_schema_responses: Arc<Mutex<HashMap<String, oneshot::Sender<NodeSchema>>>>,
    pub(crate) tokio_handle: tokio::runtime::Handle,
    pub(crate) reducer_sender: CbSender<ReducerJob>,
    reducer_receiver: CbReceiver<ReducerJob>,
    pub(crate) gpu_call_sender: mpsc::Sender<GpuCallRequest>,
    gpu_call_receiver: Mutex<Option<mpsc::Receiver<GpuCallRequest>>>,
    pub(crate) modules_path: Option<PathBuf>,
    node_subscriptions: Arc<Mutex<HashMap<NodeId, Vec<SubscriptionEventSchema>>>>,
    pub(crate) node_names_by_id: Arc<Mutex<HashMap<NodeId, String>>>,
    pub(crate) replica_bindings: Arc<Mutex<Vec<ReplicaBinding>>>,
    /// Logical node key for `table` / `module.table` ACL paths (usually the node UUID; a registry
    /// display name may override when wired in from the CLI).
    pub(crate) local_display_name: Mutex<Option<String>>,
    pub(crate) emitted_replica_sync_events: Arc<Mutex<HashSet<String>>>,
    pub(crate) file_watchers: Arc<Mutex<Vec<RecommendedWatcher>>>,
    pub(crate) call_sequence: AtomicU64,
    pub(crate) active_subscription_count: AtomicI32,
    pub(crate) timer_tx: tokio::sync::mpsc::UnboundedSender<(std::time::Instant, String, String)>,
    timer_rx: Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<(std::time::Instant, String, String)>>>,
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
        event_sender: UnboundedSender<(EventInstance, Option<CompletionToken>)>,
        network_handle: NetworkHandle,
        audio_state: Arc<Mutex<AudioState>>,
        gpu: Arc<Mutex<Option<GpuState>>>,
        run_app_notify: Arc<Notify>,
        logger: Logger,
        reducer_sender: CbSender<ReducerJob>,
        reducer_receiver: CbReceiver<ReducerJob>,
        local_display_name: Option<String>,
    ) -> Result<Self, IntersticeError> {
        // reducer_sender/receiver are pre-created by the Node so the same sender can be
        // shared directly with the Network layer, bypassing the unbounded event channel.
        let (gpu_call_sender, gpu_call_receiver) = mpsc::channel::<GpuCallRequest>();
        let (timer_tx, timer_rx) = tokio::sync::mpsc::unbounded_channel::<(std::time::Instant, String, String)>();
        let tokio_handle = tokio::runtime::Handle::current();
        let mut config = Config::new();
        config.cranelift_opt_level(wasmtime::OptLevel::Speed);
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
            engine,
            linker: Arc::new(linker),
            event_sender,
            network_handle,
            persistence: Arc::new(table_store),
            app_initialized: Arc::new(Mutex::new(false)),
            pending_query_responses: Arc::new(Mutex::new(HashMap::new())),
            pending_schema_responses: Arc::new(Mutex::new(HashMap::new())),
            reducer_sender,
            reducer_receiver,
            gpu_call_sender,
            gpu_call_receiver: Mutex::new(Some(gpu_call_receiver)),
            modules_path,
            run_app_notify,
            node_subscriptions: Arc::new(Mutex::new(HashMap::new())),
            node_names_by_id: Arc::new(Mutex::new(HashMap::new())),
            replica_bindings: Arc::new(Mutex::new(Vec::new())),
            local_display_name: Mutex::new(local_display_name),
            emitted_replica_sync_events: Arc::new(Mutex::new(HashSet::new())),
            logger,
            file_watchers: Arc::new(Mutex::new(Vec::new())),
            call_sequence: AtomicU64::new(0),
            active_subscription_count: AtomicI32::new(0),
            timer_tx,
            timer_rx: Mutex::new(Some(timer_rx)),
            tokio_handle,
        })
    }

    pub fn take_gpu_call_receiver(&self) -> mpsc::Receiver<GpuCallRequest> {
        self.gpu_call_receiver
            .lock()
            
            .take()
            .expect("Gpu call receiver already taken")
    }

    pub async fn run(runtime: Arc<Runtime>, mut event_receiver: UnboundedReceiver<(EventInstance, Option<CompletionToken>)>) {
        let (ready_tx, ready_rx) = crossbeam_channel::unbounded::<(u64, ReducerJob)>();
        let (done_tx, done_rx) = crossbeam_channel::unbounded::<u64>();

        {
            let incoming_rx = runtime.reducer_receiver.clone();
            let rt = runtime.clone();
            std::thread::Builder::new()
                .name("reducer-scheduler".to_string())
                .spawn(move || {
                    let mut scheduler = crate::runtime::scheduler::ReducerScheduler::<ReducerJob>::new();
                    loop {
                        crossbeam_channel::select! {
                            recv(incoming_rx) -> msg => {
                                let job = match msg {
                                    Ok(job) => job,
                                    Err(_) => break,
                                };
                                let accesses = reducer_accesses(&rt, &job.module_name, &job.reducer_name);
                                if let Some(runnable) = scheduler.enqueue(job, accesses) {
                                    if ready_tx.send((runnable.id, runnable.payload)).is_err() {
                                        return;
                                    }
                                }
                            }
                            recv(done_rx) -> msg => {
                                let done_id = match msg {
                                    Ok(id) => id,
                                    Err(_) => break,
                                };
                                for runnable in scheduler.complete(done_id) {
                                    if ready_tx.send((runnable.id, runnable.payload)).is_err() {
                                        return;
                                    }
                                }
                            }
                        }
                    }
                })
                .expect("Failed to spawn reducer scheduler thread");
        }

        // crossbeam Receiver is Clone — each thread gets its own handle to the same
        // MPMC queue with no mutex overhead.
        let num_reducer_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .min(8);
        for i in 0..num_reducer_threads {
            let receiver = ready_rx.clone();
            let done = done_tx.clone();
            let rt = runtime.clone();
            std::thread::Builder::new()
                .name(format!("wasm-reducer-{}", i))
                .spawn(move || {
                    loop {
                        let (job_id, job) = match receiver.recv() {
                            Ok(j) => j,
                            Err(_) => break,
                        };
                        // Fork the token into TLS so that event dispatch (which
                        // runs in a different async task) can attach its own fork
                        // by reading the token out of the EventInstance.  We use
                        // fork() — not clone() — so the count is incremented and
                        // the signal won't fire until both this guard AND all
                        // dispatched-event forks have been dropped.
                        let tls_fork = job.completion.as_ref().map(|t| t.fork());
                        ACTIVE_COMPLETION.with(|c| *c.borrow_mut() = tls_fork);
                        // Keep the original as the guard for THIS job's share.
                        let _completion_guard = job.completion;
                        let _ = rt.call_reducer(
                            &job.module_name,
                            &job.reducer_name,
                            job.input,
                            job.caller_node_id,
                        );
                        // Clear TLS *before* the guard drops so any event forks
                        // made during call_reducer are counted, and the guard's
                        // drop is the last decrement if no events were dispatched.
                        ACTIVE_COMPLETION.with(|c| *c.borrow_mut() = None);
                        let _ = done.send(job_id);
                    }
                })
                .expect("Failed to spawn wasm-reducer thread");
        }

        // Async flush thread: every 10ms fsyncs logged WAL entries AND drains the
        // dirty-stateful map to disk.  Both operations are bounded-latency by design.
        {
            let wal_store = runtime.persistence.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_millis(10));
                wal_store.flush_wal();
                wal_store.flush_stateful();
            });
        }

        // Timer task pool: single task manages all scheduled timers via a min-heap,
        // avoiding the overhead of spawning a task per ctx.schedule() call.
        let timer_rx = runtime.timer_rx.lock().take().expect("timer_rx already taken");
        let timer_reducer_sender = runtime.reducer_sender.clone();
        let timer_node_id = runtime.network_handle.node_id;
        tokio::spawn(async move {
            let mut rx = timer_rx;
            let mut heap: std::collections::BinaryHeap<std::cmp::Reverse<(std::time::Instant, String, String)>> =
                std::collections::BinaryHeap::new();
            loop {
                let sleep_dur = if let Some(std::cmp::Reverse((wake_at, _, _))) = heap.peek() {
                    let now = std::time::Instant::now();
                    if *wake_at <= now {
                        std::time::Duration::ZERO
                    } else {
                        *wake_at - now
                    }
                } else {
                    std::time::Duration::from_secs(3600)
                };

                tokio::select! {
                    _ = tokio::time::sleep(sleep_dur) => {
                        let now = std::time::Instant::now();
                        while let Some(std::cmp::Reverse((wake_at, _, _))) = heap.peek() {
                            if *wake_at > now { break; }
                            let std::cmp::Reverse((_, module_name, reducer_name)) = heap.pop().unwrap();
                            let _ = timer_reducer_sender.try_send(crate::runtime::reducer::ReducerJob {
                                module_name,
                                reducer_name,
                                input: interstice_abi::IntersticeValue::Vec(vec![]),
                                caller_node_id: timer_node_id,
                                completion: None,
                            });
                        }
                    }
                    msg = rx.recv() => {
                        match msg {
                            Some((wake_at, module_name, reducer_name)) => {
                                heap.push(std::cmp::Reverse((wake_at, module_name, reducer_name)));
                            }
                            None => break,
                        }
                    }
                }
            }
        });

        while let Some((event, token)) = event_receiver.recv().await {
            Runtime::handle_event(runtime.clone(), event, token, &mut event_receiver).await;
        }
    }

    async fn handle_event(
        runtime: Arc<Runtime>,
        event: EventInstance,
        completion_token: Option<CompletionToken>,
        _event_receiver: &mut UnboundedReceiver<(EventInstance, Option<CompletionToken>)>,
    ) {
        // Helper: fork the token once per subscriber.  The original token from
        // the channel is dropped at the end of handle_event — each subscriber's
        // ReducerJob holds its own fork, and the signal fires when all of them
        // (and their cascades) complete.
        let fork_token = |t: &Option<CompletionToken>| t.as_ref().map(|t| t.fork());
        match event {
            EventInstance::RequestAppInitialization => {
                let app_initialized = runtime.app_initialized.lock();
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
                let mut app_initialized = runtime.app_initialized.lock();
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
                // Use try_send so a full queue drops this call rather than blocking
                // the event loop (which would also stall queries and other events).
                let _ = runtime.reducer_sender.try_send(ReducerJob {
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
                tokio::task::spawn_blocking(move || {
                    let result = match runtime
                        .call_query(&module_name, &query_name, input, requesting_node_id)
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
                    
                    .remove(&request_id)
                {
                    let _ = sender.send(result);
                }
            }
            EventInstance::RemoteSchemaResponse { request_id, schema } => {
                if let Some(sender) = runtime
                    .pending_schema_responses
                    .lock()
                    
                    .remove(&request_id)
                {
                    let _ = sender.send(schema);
                }
            }
            EventInstance::RequestSubscription {
                requesting_node_id,
                event,
            } => {
                let mut subscriptions_by_node = runtime.node_subscriptions.lock();
                let subscriptions = subscriptions_by_node
                    .entry(requesting_node_id)
                    .or_insert(Vec::new());
                if !subscriptions.contains(&event) {
                    subscriptions.push(event);
                    runtime.active_subscription_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
            EventInstance::RequestUnsubscription {
                requesting_node_id,
                event,
            } => {
                let mut subscriptions_by_node = runtime.node_subscriptions.lock();
                if let Some(subscriptions) = subscriptions_by_node.get_mut(&requesting_node_id) {
                    let before_len = subscriptions.len();
                    subscriptions.retain(|existing| existing != &event);
                    let removed = before_len - subscriptions.len();
                    if removed > 0 {
                        runtime.active_subscription_count.fetch_sub(removed as i32, std::sync::atomic::Ordering::Relaxed);
                    }
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
                    let modules = runtime.modules.lock();
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

                        let tables = module.tables.lock();
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

                    let _ = runtime.event_sender.send((EventInstance::Module(
                        ModuleEvent::PublishRequest {
                            node_id: source_node_id.to_string(),
                            module_name,
                            wasm_binary,
                        },
                    ), None));
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
                    
                    .contains_key(&Authority::Module)
                {
                    let _ = runtime.event_sender.send((EventInstance::Module(
                        ModuleEvent::RemoveRequest {
                            node_id: source_node_id.to_string(),
                            module_name,
                        },
                    ), None));
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
                    let _ = runtime.invoke_subscription(sub, event.clone(), fork_token(&completion_token));
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
                    let _ = runtime.invoke_subscription(sub, event.clone(), fork_token(&completion_token));
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
                    let _ = runtime.invoke_subscription(sub, event.clone(), fork_token(&completion_token));
                }
            }
            event => {
                let triggered = runtime.find_subscriptions(&event).unwrap();

                for sub in triggered {
                    let _ = runtime.invoke_subscription(sub, event.clone(), fork_token(&completion_token));
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
                
                .values()
                .map(|m| (*m.schema).clone())
                .collect(),
        }
    }

    pub fn replay(&self) -> Result<(), IntersticeError> {
        let module_entries = self
            .modules
            .lock()
            
            .iter()
            .map(|(name, module)| (name.clone(), Arc::clone(module)))
            .collect::<Vec<_>>();

        for (module_name, module) in module_entries {
            let mut tables = module.tables.lock();
            for table in tables.values_mut() {
                self.persistence.restore_table(&module_name, table)?;
            }
        }

        Ok(())
    }
}

fn reducer_accesses(
    runtime: &Arc<Runtime>,
    module_name: &str,
    reducer_name: &str,
) -> Vec<crate::runtime::scheduler::TableAccess> {
    let modules = runtime.modules.lock();
    let Some(module) = modules.get(module_name) else {
        return Vec::new();
    };
    let Some(reducer) = module
        .schema
        .reducers
        .iter()
        .find(|r| r.name == reducer_name)
    else {
        return Vec::new();
    };

    let local = runtime.local_display_name.lock().clone();
    let expand_vec = |v: &[String]| -> Vec<String> {
        v.iter()
            .filter_map(|e| {
                crate::runtime::table_access::normalize_user_table_ref(
                    e,
                    local.as_deref(),
                    module_name,
                )
                .ok()
            })
            .collect()
    };

    let mut out = Vec::new();
    for table in expand_vec(&reducer.reads) {
        out.push(crate::runtime::scheduler::TableAccess {
            table_name: table,
            op: crate::runtime::scheduler::TableOp::Read,
        });
    }
    for table in expand_vec(&reducer.inserts) {
        out.push(crate::runtime::scheduler::TableAccess {
            table_name: table,
            op: crate::runtime::scheduler::TableOp::Insert,
        });
    }
    for table in expand_vec(&reducer.updates) {
        out.push(crate::runtime::scheduler::TableAccess {
            table_name: table,
            op: crate::runtime::scheduler::TableOp::Update,
        });
    }
    for table in expand_vec(&reducer.deletes) {
        out.push(crate::runtime::scheduler::TableAccess {
            table_name: table,
            op: crate::runtime::scheduler::TableOp::Delete,
        });
    }
    out
}
