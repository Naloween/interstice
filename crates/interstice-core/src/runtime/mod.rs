mod authority;
pub mod blob;
mod deterministic_random;
pub mod event;
pub mod host_calls;
pub mod migration;
pub mod profile;
pub mod module;
mod query;
pub mod reducer;
pub(crate) mod view;
mod scheduler;
pub mod table;
pub mod transaction;
mod wasm;

pub(crate) use authority::AuthorityEntry;

use crate::runtime::migration::MigrationPolicy;
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
            network::NetworkState,
        },
        module::Module,
        reducer::{ACTIVE_COMPLETION, CompletionToken, Job, ReducerJob, ReplicaChange},
        wasm::{StoreState, linker::define_host_calls},
    },
};
use interstice_abi::{
    Authority, IntersticeValue, ModuleEvent, NodeSchema, SubscriptionEventSchema,
};
use notify::RecommendedWatcher;
use std::sync::atomic::AtomicI32;
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

/// Maximum wasm operand/call stack a reducer may use. Raised well above wasmtime's
/// 512 KiB default so recursive module workloads (e.g. the browser example's DOM and
/// layout walk over deeply nested pages) don't trap on ordinary content.
const WASM_STACK_SIZE: usize = 8 * 1024 * 1024;

pub struct Runtime {
    pub(crate) node_id: NodeId,
    /// Owners, call rules and the file root (DESIGN §7.4).
    pub(crate) security: Arc<crate::security::Security>,
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
        Arc<Mutex<HashMap<String, std_mpsc::Sender<Result<IntersticeValue, String>>>>>,
    pub(crate) pending_schema_responses: Arc<Mutex<HashMap<String, oneshot::Sender<NodeSchema>>>>,
    pub(crate) tokio_handle: tokio::runtime::Handle,
    pub(crate) reducer_sender: CbSender<Job>,
    reducer_receiver: CbReceiver<Job>,
    /// Ordered staging queue feeding `reducer_sender`. Producers push here
    /// inline — it's unbounded so the send never blocks the async runtime — and
    /// a single forwarder thread (spawned in `run`) drains it in order and does
    /// the blocking, never-drop bounded send into `reducer_sender`.
    ///
    /// This exists purely to preserve emission order. The previous approach
    /// spawned one OS thread per send to avoid blocking on the bounded channel;
    /// those threads raced, so e.g. a connection's final `Received` event could
    /// reach the scheduler *after* its `Closed`, truncating the buffered body.
    pub(crate) reducer_ingress: CbSender<Job>,
    reducer_ingress_rx: Mutex<Option<CbReceiver<Job>>>,
    pub(crate) gpu_call_sender: mpsc::Sender<GpuCallRequest>,
    gpu_call_receiver: Mutex<Option<mpsc::Receiver<GpuCallRequest>>>,
    pub(crate) modules_path: Option<PathBuf>,
    pub(crate) node_subscriptions: Arc<Mutex<HashMap<NodeId, Vec<SubscriptionEventSchema>>>>,
    /// Function-view results (DESIGN §7.5).
    pub(crate) view_cache: view::ViewCache,
    pub(crate) node_names_by_id: Arc<Mutex<HashMap<NodeId, String>>>,
    pub(crate) replica_bindings: Arc<Mutex<Vec<ReplicaBinding>>>,
    pub(crate) emitted_replica_sync_events: Arc<Mutex<HashSet<String>>>,
    pub(crate) file_watchers: Arc<Mutex<Vec<RecommendedWatcher>>>,
    pub(crate) network_state: Arc<NetworkState>,
    /// Transaction ids and timestamps (DESIGN §6.5).
    pub(crate) tx_clock: crate::runtime::deterministic_random::TxClock,
    pub(crate) active_subscription_count: AtomicI32,
    pub(crate) timer_tx: tokio::sync::mpsc::UnboundedSender<(std::time::Instant, String, String)>,
    timer_rx: Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<(std::time::Instant, String, String)>>>,
    /// Sorted [`TableAccess`] slices per module → reducer for the scheduler hot path.
    /// Nested maps allow `get` with `&str` without allocating lookup keys.
    reducer_access_cache:
        parking_lot::Mutex<HashMap<String, HashMap<String, Arc<[scheduler::TableAccess]>>>>,
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
        reducer_sender: CbSender<Job>,
        reducer_receiver: CbReceiver<Job>,
    ) -> Result<Self, IntersticeError> {
        // reducer_sender/receiver are pre-created by the Node so the same sender can be
        // shared directly with the Network layer, bypassing the unbounded event channel.
        let (gpu_call_sender, gpu_call_receiver) = mpsc::channel::<GpuCallRequest>();
        let (timer_tx, timer_rx) = tokio::sync::mpsc::unbounded_channel::<(std::time::Instant, String, String)>();
        let tokio_handle = tokio::runtime::Handle::current();
        let mut config = Config::new();
        config.cranelift_opt_level(wasmtime::OptLevel::Speed);
        // Deeply nested content (e.g. a Wikipedia article's tables-within-tables) drives
        // the module's recursive DOM/layout walk far past wasmtime's default 512 KiB wasm
        // stack, trapping the reducer. Raise the wasm stack ceiling to 8 MiB; the
        // wasm-reducer threads below are given a matching native stack (wasm runs on the
        // calling thread's stack, so the native stack must comfortably exceed this).
        config.max_wasm_stack(WASM_STACK_SIZE);
        let engine = Arc::new(Engine::new(&config).unwrap());
        let mut linker = Linker::new(&engine);
        define_host_calls(&mut linker).map_err(|err| {
            IntersticeError::Internal(format!("Couldn't add host calls to the linker: {}", err))
        })?;
        let (reducer_ingress, reducer_ingress_rx) = crossbeam_channel::unbounded::<Job>();
        let files_root = modules_path
            .as_ref()
            .and_then(|p| p.parent())
            .map(|p| p.join("files"))
            .unwrap_or_else(|| PathBuf::from("files"));
        Ok(Self {
            security: Arc::new(crate::security::Security::new(node_id, files_root)),
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
            reducer_ingress,
            reducer_ingress_rx: Mutex::new(Some(reducer_ingress_rx)),
            gpu_call_sender,
            gpu_call_receiver: Mutex::new(Some(gpu_call_receiver)),
            modules_path,
            run_app_notify,
            node_subscriptions: Arc::new(Mutex::new(HashMap::new())),
            view_cache: Default::default(),
            node_names_by_id: Arc::new(Mutex::new(HashMap::new())),
            replica_bindings: Arc::new(Mutex::new(Vec::new())),
            emitted_replica_sync_events: Arc::new(Mutex::new(HashSet::new())),
            logger,
            file_watchers: Arc::new(Mutex::new(Vec::new())),
            network_state: Arc::new(NetworkState::new()),
            tx_clock: crate::runtime::deterministic_random::TxClock::new(),
            active_subscription_count: AtomicI32::new(0),
            timer_tx,
            timer_rx: Mutex::new(Some(timer_rx)),
            tokio_handle,
            reducer_access_cache: parking_lot::Mutex::new(HashMap::new()),
        })
    }

    pub(crate) fn clear_reducer_access_cache(&self) {
        self.reducer_access_cache.lock().clear();
    }

    pub fn take_gpu_call_receiver(&self) -> mpsc::Receiver<GpuCallRequest> {
        self.gpu_call_receiver
            .lock()
            
            .take()
            .expect("Gpu call receiver already taken")
    }

    pub async fn run(runtime: Arc<Runtime>, mut event_receiver: UnboundedReceiver<(EventInstance, Option<CompletionToken>)>) {
        let (ready_tx, ready_rx) =
            crossbeam_channel::unbounded::<(u64, Job, Option<std::time::Instant>)>();
        // The scheduler is shared (DESIGN §6.1): this thread admits incoming
        // jobs, and a worker that finishes a job completes it itself, keeps
        // the first job it releases and runs it without a thread hop.
        let scheduler = Arc::new(parking_lot::Mutex::new(
            crate::runtime::scheduler::ReducerScheduler::<Job>::new(),
        ));

        {
            let incoming_rx = runtime.reducer_receiver.clone();
            let rt = runtime.clone();
            let scheduler = scheduler.clone();
            let ready_tx = ready_tx.clone();
            std::thread::Builder::new()
                .name("reducer-scheduler".to_string())
                .spawn(move || {
                    while let Ok(job) = incoming_rx.recv() {
                        let accesses = job_accesses(&rt, &job);
                        let runnable = scheduler.lock().enqueue(job, accesses);
                        if let Some(runnable) = runnable {
                            profile::since(profile::Phase::Blocked, runnable.born);
                            if ready_tx.send((runnable.id, runnable.payload, profile::now())).is_err() {
                                return;
                            }
                        }
                    }
                })
                .expect("Failed to spawn reducer scheduler thread");
        }

        // Ordered ingress forwarder: drains the unbounded staging queue
        // (`reducer_ingress`) in FIFO order and does the blocking, never-drop
        // send into the bounded `reducer_sender`. A single thread guarantees
        // events reach the scheduler in their emission order — critical for
        // e.g. a connection's `Received` chunks preceding its `Closed`.
        {
            let staging_rx = runtime
                .reducer_ingress_rx
                .lock()
                .take()
                .expect("reducer_ingress_rx already taken");
            let bounded_tx = runtime.reducer_sender.clone();
            std::thread::Builder::new()
                .name("reducer-ingress-forwarder".to_string())
                .spawn(move || {
                    while let Ok(job) = staging_rx.recv() {
                        if bounded_tx.send(job).is_err() {
                            break;
                        }
                    }
                })
                .expect("Failed to spawn reducer ingress forwarder thread");
        }

        // crossbeam Receiver is Clone — each thread gets its own handle to the same
        // MPMC queue with no mutex overhead.
        let num_reducer_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .min(8);
        for i in 0..num_reducer_threads {
            let receiver = ready_rx.clone();
            let ready_tx = ready_tx.clone();
            let scheduler = scheduler.clone();
            let rt = runtime.clone();
            std::thread::Builder::new()
                .name(format!("wasm-reducer-{}", i))
                // Native stack must exceed max_wasm_stack (8 MiB) with headroom for host
                // trampolines and re-entrant wasm→host→wasm reducer calls.
                .stack_size(WASM_STACK_SIZE + 8 * 1024 * 1024)
                .spawn(move || {
                    let mut next = None;
                    loop {
                        let (job_id, job, ready_at) = match next.take() {
                            Some(j) => j,
                            None => match receiver.recv() {
                                Ok(j) => j,
                                Err(_) => break,
                            },
                        };
                        profile::since(profile::Phase::Ready, ready_at);
                        // Fork the token into TLS so that everything this job
                        // emits (events, call batches) holds its own fork. We use
                        // fork() — not clone() — so the count is incremented and
                        // the signal won't fire until both this guard AND all
                        // emitted forks have been dropped.
                        let completion = match &job {
                            Job::Reducer(j) => j.completion.as_ref(),
                            Job::Calls { completion, .. } | Job::Replica { completion, .. } => {
                                completion.as_ref()
                            }
                        };
                        let tls_fork = completion.map(|t| t.fork());
                        ACTIVE_COMPLETION.with(|c| *c.borrow_mut() = tls_fork);
                        match job {
                            Job::Reducer(job) => {
                                // Keep the original as the guard for THIS job's share.
                                let _completion_guard = job.completion;
                                let result = rt.call_reducer(
                                    &job.module_name,
                                    &job.reducer_name,
                                    job.input,
                                    job.caller_node_id,
                                    &job.caller_module_name,
                                    None,
                                );
                                if let Some(admission) = &job.admission {
                                    // No holder any more means no policy: admit.
                                    let admit = matches!(result, Ok(()) | Err(IntersticeError::ModuleNotFound(..)));
                                    admission.decide(admit);
                                    if !admit {
                                        rt.network_handle.disconnect_peer(admission.peer);
                                    }
                                }
                                if let Some(reply) = job.reply {
                                    let _reply = profile::span(profile::Phase::Reply);
                                    let outcome = result.as_ref().map(|_| ()).map_err(|e| e.to_string());
                                    match reply {
                                        crate::runtime::reducer::Reply::Network(request_id) => rt.network_handle.send_packet(
                                            job.caller_node_id,
                                            crate::network::protocol::NetworkPacket::ReducerResult { request_id, outcome },
                                        ),
                                        crate::runtime::reducer::Reply::Local(sender) => {
                                            let _ = sender.send(outcome);
                                        }
                                    }
                                }
                                if let Err(err) = result {
                                    // The target module is not loaded: a call to a
                                    // wrong name, or a job queued (e.g. an event)
                                    // for a module unloaded before it ran. Either
                                    // way the job is dropped; it is not a failure
                                    // of the module.
                                    let (what, level) = match err {
                                        IntersticeError::ModuleNotFound(..) => {
                                            ("Reducer job dropped (module not loaded)", LogLevel::Warning)
                                        }
                                        _ => ("Reducer execution failed", LogLevel::Error),
                                    };
                                    rt.logger.log(
                                        &format!("{what} for '{}.{}': {}", job.module_name, job.reducer_name, err),
                                        LogSource::Runtime,
                                        level,
                                    );
                                }
                            }
                            Job::Calls { batch, completion: _completion_guard } => {
                                rt.run_call_batch(batch);
                            }
                            Job::Replica { change, completion: _completion_guard } => {
                                rt.apply_replica_change(change);
                            }
                        }
                        // Clear TLS so any forks made during the job are counted
                        // and the guard's drop (above) or this one is the last.
                        ACTIVE_COMPLETION.with(|c| *c.borrow_mut() = None);
                        let released = scheduler.lock().complete(job_id);
                        for runnable in released {
                            profile::since(profile::Phase::Blocked, runnable.born);
                            let job = (runnable.id, runnable.payload, profile::now());
                            if next.is_none() {
                                next = Some(job);
                            } else if ready_tx.send(job).is_err() {
                                return;
                            }
                        }
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
        let timer_reducer_sender = runtime.reducer_ingress.clone();
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
                            let job = crate::runtime::reducer::ReducerJob {
                                module_name,
                                reducer_name,
                                input: interstice_abi::IntersticeValue::Vec(vec![]),
                                caller_node_id: timer_node_id,
                                caller_module_name: String::new(),
                                admission: None,
                                completion: None,
                                reply: None,
                            };
                            let _ = timer_reducer_sender.send(job.into());
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
            EventInstance::RemoteQueryCall {
                requesting_node_id,
                request_id,
                module_name,
                query_name,
                input,
            } => {
                let runtime = runtime.clone();
                tokio::task::spawn_blocking(move || {
                    match runtime.call_query(&module_name, &query_name, input, requesting_node_id, "")
                    {
                        Ok(value) => {
                            runtime.network_handle.send_packet(
                                requesting_node_id,
                                crate::network::protocol::NetworkPacket::QueryResponse {
                                    request_id,
                                    result: Ok(value),
                                },
                            );
                        }
                        Err(err) => {
                            runtime.logger.log(
                                &format!(
                                    "Remote query '{}' on module '{}' failed: {}",
                                    query_name, module_name, err
                                ),
                                LogSource::Runtime,
                                LogLevel::Error,
                            );
                            runtime.network_handle.send_packet(
                                requesting_node_id,
                                crate::network::protocol::NetworkPacket::QueryResponse {
                                    request_id,
                                    result: Err(err.to_string()),
                                },
                            );
                        }
                    }
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
                // A remote subscription reads the table: same rule as table sync (DESIGN §7.4).
                let target = match &event {
                    SubscriptionEventSchema::Insert { module_name, table_name, .. }
                    | SubscriptionEventSchema::Update { module_name, table_name, .. }
                    | SubscriptionEventSchema::Delete { module_name, table_name, .. } => {
                        Some((module_name.clone(), table_name.clone()))
                    }
                    _ => None,
                };
                let denied = match target {
                    Some((module_name, table_name)) => match runtime.modules.lock().get(&module_name) {
                        Some(module) => runtime
                            .security
                            .check_remote_table(&module.schema, &table_name, requesting_node_id)
                            .err(),
                        None => Some(IntersticeError::ModuleNotFound(module_name, "subscription".into())),
                    },
                    None => Some(IntersticeError::PermissionDenied("only table events can be subscribed to remotely".into())),
                };
                if let Some(err) = denied {
                    runtime.logger.log(
                        &format!("Subscription from {requesting_node_id} refused: {err}"),
                        LogSource::Runtime,
                        LogLevel::Warning,
                    );
                    return;
                }
                // A function view keeps a result for this peer (DESIGN §7.5).
                if let SubscriptionEventSchema::Insert { module_name, table_name, .. }
                | SubscriptionEventSchema::Update { module_name, table_name, .. }
                | SubscriptionEventSchema::Delete { module_name, table_name, .. } = &event
                {
                    let owner = runtime.modules.lock().get(module_name).cloned();
                    if let Some(owner) = owner {
                        if let Some(view) = owner.schema.view(table_name).cloned() {
                            if matches!(view.kind, interstice_abi::ViewKind::Function { .. }) {
                                let runtime = runtime.clone();
                                let viewer = Runtime::remote_viewer(requesting_node_id);
                                tokio::task::spawn_blocking(move || runtime.prime_view(&owner, &view, &viewer));
                            }
                        }
                    }
                }
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
                    let module = runtime.modules.lock().get(&module_name).cloned().ok_or_else(|| {
                        IntersticeError::ModuleNotFound(
                            module_name.clone(),
                            "When handling table sync request".to_string(),
                        )
                    });

                    module.and_then(|module| {
                        // A view syncs as the peer sees it (DESIGN §7.5).
                        if let Some(view) = module.schema.view(&table_name) {
                            runtime.security.check_remote_table(&module.schema, &table_name, requesting_node_id)?;
                            let viewer = Runtime::remote_viewer(requesting_node_id);
                            return runtime
                                .view_rows(&module, view, &viewer, false)
                                .map_err(IntersticeError::Internal);
                        }
                        module
                            .schema
                            .tables
                            .iter()
                            .find(|table| table.name == table_name)
                            .ok_or_else(|| IntersticeError::TableNotFound {
                                module_name: module_name.clone(),
                                table_name: table_name.clone(),
                            })?;
                        runtime.security.check_remote_table(&module.schema, &table_name, requesting_node_id)?;

                        let tables = module.tables.read_recursive();
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
                let change = ReplicaChange::FullSync {
                    source_node_id,
                    module_name,
                    table_name,
                    rows,
                };
                let _ = runtime.reducer_ingress.send(Job::Replica {
                    change,
                    completion: fork_token(&completion_token),
                });
            }
            EventInstance::Calls(batch) => {
                let _ = runtime.reducer_ingress.send(Job::Calls {
                    batch,
                    completion: fork_token(&completion_token),
                });
            }
            EventInstance::LoadModule {
                wasm_binary,
                source_node_id,
                policy,
                request_id,
                grant,
            } => {
                // Answer the requester (the CLI waits for it, DESIGN §3.3).
                let reply = {
                    let runtime = runtime.clone();
                    move |outcome: crate::network::protocol::LoadOutcome| {
                        if !request_id.is_empty() {
                            runtime.network_handle.send_packet(
                                source_node_id,
                                crate::network::protocol::NetworkPacket::ModuleLoadResult {
                                    request_id: request_id.clone(),
                                    outcome,
                                },
                            );
                        }
                    }
                };
                use crate::network::protocol::LoadOutcome;
                // Administration (DESIGN §7.4): the Module holder decides;
                // without one, only owners may load.
                let owner = runtime.security.is_owner(&source_node_id);
                if runtime
                    .authority_modules
                    .lock()
                    .contains_key(&Authority::Module)
                {
                    let module_name = match crate::bundle::describe_wasm(&wasm_binary) {
                        Ok(schema) => schema.name,
                        Err(err) => {
                            runtime.logger.log(
                                &format!(
                                    "Failed to decode loaded module before forwarding to module authority: {}",
                                    err
                                ),
                                LogSource::Runtime,
                                LogLevel::Error,
                            );
                            reply(LoadOutcome::Failed(err.to_string()));
                            return;
                        }
                    };

                    let _ = runtime.event_sender.send((EventInstance::Module(
                        ModuleEvent::LoadRequest {
                            node_id: source_node_id.to_string(),
                            owner,
                            module_name,
                            wasm_binary,
                        },
                    ), None));
                    reply(LoadOutcome::Forwarded);
                } else if !owner {
                    runtime.logger.log(
                        &format!("Load refused: {source_node_id} is not an owner of this node"),
                        LogSource::Runtime,
                        LogLevel::Warning,
                    );
                    reply(LoadOutcome::Failed("Permission denied: not an owner of this node".into()));
                } else {
                    let runtime_cloned = runtime.clone();
                    tokio::task::spawn_local(async move {
                        let outcome = Runtime::update_module(runtime_cloned, wasm_binary, policy, grant).await;
                        reply(outcome);
                    });
                }
            }
            EventInstance::UnloadModule { module_name, source_node_id } => {
                // Unload keeps the module's data on disk.
                let owner = runtime.security.is_owner(&source_node_id);
                if runtime.authority_modules.lock().contains_key(&Authority::Module) {
                    let _ = runtime.event_sender.send((EventInstance::Module(
                        ModuleEvent::UnloadRequest {
                            node_id: source_node_id.to_string(),
                            owner,
                            module_name,
                        },
                    ), None));
                } else if owner {
                    Runtime::unload_module(runtime.clone(), &module_name);
                } else {
                    runtime.logger.log(
                        &format!("Unload of '{module_name}' refused: {source_node_id} is not an owner"),
                        LogSource::Runtime,
                        LogLevel::Warning,
                    );
                }
            }
            EventInstance::RemoveModule {
                module_name,
                source_node_id,
            } => {
                let owner = runtime.security.is_owner(&source_node_id);
                if runtime
                    .authority_modules
                    .lock()
                    .contains_key(&Authority::Module)
                {
                    let _ = runtime.event_sender.send((EventInstance::Module(
                        ModuleEvent::RemoveRequest {
                            node_id: source_node_id.to_string(),
                            owner,
                            module_name,
                        },
                    ), None));
                } else if owner {
                    Runtime::remove_module(runtime.clone(), &module_name);
                } else {
                    runtime.logger.log(
                        &format!("Removal of '{module_name}' refused: {source_node_id} is not an owner"),
                        LogSource::Runtime,
                        LogLevel::Warning,
                    );
                }
            }
            EventInstance::SchemaRequest {
                requesting_node_id,
                request_id,
                node_name,
            } => {
                // Non-owners see public modules only (DESIGN §7.4).
                let schema = runtime.build_node_schema(node_name);
                let schema = if runtime.security.is_owner(&requesting_node_id) { schema } else { schema.to_remote() };
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
                    // Enqueued before the subscriber jobs below, which declare
                    // a read on the replica and so run after it (DESIGN §6.6).
                    let change = ReplicaChange::Insert {
                        source_node_id,
                        module_name: module_name.clone(),
                        table_name: table_name.clone(),
                        row: inserted_row.clone(),
                    };
                    let _ = runtime.reducer_ingress.send(Job::Replica {
                        change,
                        completion: fork_token(&completion_token),
                    });
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
                // Views over this table, as each subscriber sees them (DESIGN §7.5).
                for (target, view_event) in runtime.view_events(&event) {
                    let _ = runtime.invoke_subscription(target, view_event, fork_token(&completion_token));
                }
                Runtime::refresh_function_views(&runtime, &event);
            }
            EventInstance::TableUpdateEvent {
                source_node_id,
                module_name,
                table_name,
                old_row,
                new_row,
            } => {
                if let Some(source_node_id) = source_node_id {
                    let change = ReplicaChange::Update {
                        source_node_id,
                        module_name: module_name.clone(),
                        table_name: table_name.clone(),
                        old_row: old_row.clone(),
                        new_row: new_row.clone(),
                    };
                    let _ = runtime.reducer_ingress.send(Job::Replica {
                        change,
                        completion: fork_token(&completion_token),
                    });
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
                // Views over this table, as each subscriber sees them (DESIGN §7.5).
                for (target, view_event) in runtime.view_events(&event) {
                    let _ = runtime.invoke_subscription(target, view_event, fork_token(&completion_token));
                }
                Runtime::refresh_function_views(&runtime, &event);
            }
            EventInstance::TableDeleteEvent {
                source_node_id,
                module_name,
                table_name,
                deleted_row,
            } => {
                if let Some(source_node_id) = source_node_id {
                    let change = ReplicaChange::Delete {
                        source_node_id,
                        module_name: module_name.clone(),
                        table_name: table_name.clone(),
                        row: deleted_row.clone(),
                    };
                    let _ = runtime.reducer_ingress.send(Job::Replica {
                        change,
                        completion: fork_token(&completion_token),
                    });
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
                // Views over this table, as each subscriber sees them (DESIGN §7.5).
                for (target, view_event) in runtime.view_events(&event) {
                    let _ = runtime.invoke_subscription(target, view_event, fork_token(&completion_token));
                }
                Runtime::refresh_function_views(&runtime, &event);
            }
            EventInstance::Targeted { target, event } => {
                let _ = runtime.invoke_subscription(target, *event, fork_token(&completion_token));
            }
            event => {
                let triggered = runtime.find_subscriptions(&event).unwrap();
                // A connecting peer waits for the Module holder's `connect`
                // handler; with none, it is admitted now (DESIGN §7.4).
                if let EventInstance::NodeConnect { admission: Some(admission), .. } = &event {
                    let holder = runtime
                        .authority_modules
                        .lock()
                        .get(&Authority::Module)
                        .map(|e| e.module_name().to_string());
                    let gated = triggered.iter().any(|t| {
                        matches!(t, event::SubscriptionTarget::Local { module, .. } if Some(module) == holder.as_ref())
                    });
                    if !gated {
                        admission.decide(true);
                    }
                }

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
}

/// Scheduler key for a declared table ref of `module_name`: `Current` module
/// selections become the module's own name and names are lower-cased, so
/// every job touching a table uses the same key for it.
fn normalized_table_key(module_name: &str, table_ref: &interstice_abi::ReducerTableRef) -> scheduler::AccessKey {
    use interstice_abi::{ModuleSelection, NodeSelection, ReducerTableRef};
    let node_selection = match &table_ref.node_selection {
        NodeSelection::Current => NodeSelection::Current,
        NodeSelection::Other(n) => NodeSelection::Other(n.to_lowercase()),
    };
    let module_selection = match (&node_selection, &table_ref.module_selection) {
        (NodeSelection::Current, ModuleSelection::Current) => {
            ModuleSelection::Other(module_name.to_lowercase())
        }
        (_, ModuleSelection::Current) => ModuleSelection::Current,
        (_, ModuleSelection::Other(m)) => ModuleSelection::Other(m.to_lowercase()),
    };
    scheduler::AccessKey::Table(ReducerTableRef {
        node_selection,
        module_selection,
        table_name: table_ref.table_name.to_lowercase(),
    })
}

/// The access set of any job (DESIGN §6.6, Jobs).
fn job_accesses(runtime: &Arc<Runtime>, job: &Job) -> Arc<[scheduler::TableAccess]> {
    match job {
        Job::Reducer(j) => reducer_accesses_cached(runtime, &j.module_name, &j.reducer_name),
        Job::Calls { batch, .. } => {
            let mut out: Vec<scheduler::TableAccess> = Vec::new();
            let mut callees: Vec<&str> = Vec::new();
            for call in &batch.calls {
                if callees.contains(&call.module_name.as_str()) {
                    continue;
                }
                callees.push(&call.module_name);
                out.push(scheduler::TableAccess {
                    key: scheduler::AccessKey::Channel {
                        from: batch.caller_module_name.clone(),
                        to: call.module_name.clone(),
                    },
                    op: scheduler::TableOp::Update,
                });
            }
            let mut reducers: Vec<(&str, &str)> = Vec::new();
            for call in &batch.calls {
                let key = (call.module_name.as_str(), call.reducer_name.as_str());
                if !reducers.contains(&key) {
                    reducers.push(key);
                    out.extend(
                        reducer_accesses_cached(runtime, key.0, key.1).iter().cloned(),
                    );
                }
            }
            scheduler::sort_table_accesses(&mut out);
            out.dedup();
            Arc::from(out.into_boxed_slice())
        }
        Job::Replica { change, .. } => {
            let (source_node_id, module_name, table_name) = change.source();
            let mut out: Vec<scheduler::TableAccess> = runtime
                .replica_bindings
                .lock()
                .iter()
                .filter(|b| {
                    b.source_node_id == source_node_id
                        && b.source_module_name == module_name
                        && b.source_table_name == table_name
                })
                .map(|b| scheduler::TableAccess {
                    key: scheduler::AccessKey::Table(interstice_abi::ReducerTableRef {
                        node_selection: interstice_abi::NodeSelection::Other(
                            b.source_node_name.to_lowercase(),
                        ),
                        module_selection: interstice_abi::ModuleSelection::Other(
                            b.source_module_name.to_lowercase(),
                        ),
                        table_name: b.source_table_name.to_lowercase(),
                    }),
                    op: scheduler::TableOp::Update,
                })
                .collect();
            scheduler::sort_table_accesses(&mut out);
            out.dedup();
            Arc::from(out.into_boxed_slice())
        }
    }
}

/// The scheduler keys of the tables behind `table_ref`, if it names a view
/// of a module on this node.
fn view_base_tables(
    modules: &HashMap<String, Arc<Module>>,
    module_name: &str,
    table_ref: &interstice_abi::ReducerTableRef,
) -> Vec<scheduler::AccessKey> {
    use interstice_abi::{ModuleSelection, NodeSelection, ViewKind};
    if table_ref.node_selection != NodeSelection::Current {
        return Vec::new();
    }
    let target = match &table_ref.module_selection {
        ModuleSelection::Current => module_name,
        ModuleSelection::Other(m) => m.as_str(),
    };
    let Some(owner) = modules.values().find(|m| m.schema.name.eq_ignore_ascii_case(target)) else {
        return Vec::new();
    };
    let Some(view) = owner.schema.views.iter().find(|v| v.name.eq_ignore_ascii_case(&table_ref.table_name)) else {
        return Vec::new();
    };
    let owner_name = owner.schema.name.clone();
    match &view.kind {
        ViewKind::Filter { table, .. } => vec![normalized_table_key(
            &owner_name,
            &interstice_abi::ReducerTableRef {
                node_selection: NodeSelection::Current,
                module_selection: ModuleSelection::Current,
                table_name: table.clone(),
            },
        )],
        ViewKind::Function { reads, .. } => reads.iter().map(|r| normalized_table_key(&owner_name, r)).collect(),
    }
}

fn reducer_accesses_uncached(
    runtime: &Arc<Runtime>,
    module_name: &str,
    reducer_name: &str,
) -> Vec<scheduler::TableAccess> {
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

    let mut out = Vec::new();
    for table_ref in &reducer.reads {
        out.push(scheduler::TableAccess {
            key: normalized_table_key(module_name, table_ref),
            op: scheduler::TableOp::Read,
        });
        // Reading a view reads the tables behind it (DESIGN §7.5).
        for base in view_base_tables(&modules, module_name, table_ref) {
            out.push(scheduler::TableAccess { key: base, op: scheduler::TableOp::Read });
        }
    }
    for table_ref in &reducer.inserts {
        out.push(scheduler::TableAccess {
            key: normalized_table_key(module_name, table_ref),
            op: scheduler::TableOp::Insert,
        });
    }
    for table_ref in &reducer.updates {
        out.push(scheduler::TableAccess {
            key: normalized_table_key(module_name, table_ref),
            op: scheduler::TableOp::Update,
        });
    }
    for table_ref in &reducer.deletes {
        out.push(scheduler::TableAccess {
            key: normalized_table_key(module_name, table_ref),
            op: scheduler::TableOp::Delete,
        });
    }
    out
}

fn reducer_accesses_cached(
    runtime: &Arc<Runtime>,
    module_name: &str,
    reducer_name: &str,
) -> Arc<[scheduler::TableAccess]> {
    {
        let cache = runtime.reducer_access_cache.lock();
        if let Some(hit) = cache
            .get(module_name)
            .and_then(|m| m.get(reducer_name))
        {
            return hit.clone();
        }
    }
    let mut v = reducer_accesses_uncached(runtime, module_name, reducer_name);
    scheduler::sort_table_accesses(&mut v);
    let arc: Arc<[scheduler::TableAccess]> = Arc::from(v.into_boxed_slice());
    runtime
        .reducer_access_cache
        .lock()
        .entry(module_name.to_string())
        .or_default()
        .insert(reducer_name.to_string(), arc.clone());
    arc
}

impl Runtime {
    /// Load `wasm_binary`; when that module is already loaded this is an
    /// update (DESIGN §3.3): the plan is checked against the running
    /// version first, and if the new version then fails to load, the old
    /// one is loaded back.
    pub(crate) async fn update_module(
        runtime: Arc<Self>,
        wasm_binary: Vec<u8>,
        policy: MigrationPolicy,
        grant: bool,
    ) -> crate::network::protocol::LoadOutcome {
        Self::update_module_named(runtime, wasm_binary, policy, grant).await.1
    }

    /// [`Self::update_module`], with the module's name (empty when the
    /// bytes don't compile).
    pub(crate) async fn update_module_named(
        runtime: Arc<Self>,
        wasm_binary: Vec<u8>,
        policy: MigrationPolicy,
        grant: bool,
    ) -> (String, crate::network::protocol::LoadOutcome) {
        let mut name = String::new();
        let outcome = Self::update_module_inner(runtime, wasm_binary, policy, grant, &mut name).await;
        (name, outcome)
    }

    async fn update_module_inner(
        runtime: Arc<Self>,
        wasm_binary: Vec<u8>,
        policy: MigrationPolicy,
        grant: bool,
        out_name: &mut String,
    ) -> crate::network::protocol::LoadOutcome {
        use crate::network::protocol::LoadOutcome;
        let log_err = |runtime: &Arc<Self>, text: String| {
            runtime.logger.log(&text, LogSource::Runtime, LogLevel::Error);
        };
        let module = match Module::from_bytes(runtime.clone(), &wasm_binary).await {
            Ok(module) => module,
            Err(err) => {
                log_err(&runtime, format!("Failed to decode loaded module bytes: {}", err));
                return LoadOutcome::Failed(err.to_string());
            }
        };
        let name = module.schema.name.clone();
        *out_name = name.clone();
        // Authorities not granted before need the requester's grant (DESIGN §7.4).
        if !grant {
            let missing = crate::runtime::module::ungranted_authorities(&runtime, &module.schema);
            if !missing.is_empty() {
                return LoadOutcome::NeedsGrant {
                    module: name,
                    authorities: missing.iter().map(|a| format!("{a:?}")).collect(),
                };
            }
        }
        let running = runtime.modules.lock().get(&name).map(|m| m.wasm_bytes.clone());
        if running.as_deref() == Some(wasm_binary.as_slice()) {
            // The same bytes are running already: nothing to do.
            return LoadOutcome::Loaded { plan: String::new() };
        }
        if let Some(old_wasm) = running {
            // Check the plan while the old version still runs.
            if policy == MigrationPolicy::Safe {
                runtime.persistence.flush_stateful();
                runtime.persistence.flush_wal();
                match Runtime::check_migration(&runtime, &module).await {
                    Ok(None) => {}
                    Ok(Some(plan)) => {
                        runtime.logger.log(
                            &format!("Update of '{name}' refused, the old version keeps running.\n{plan}"),
                            LogSource::Runtime,
                            LogLevel::Warning,
                        );
                        return LoadOutcome::NeedsDecision { plan };
                    }
                    Err(err) => return LoadOutcome::Failed(err.to_string()),
                }
            }
            Runtime::unload_module(runtime.clone(), &name);
            return match Runtime::load_module_with(runtime.clone(), module, policy).await {
                Ok((_, plan)) => LoadOutcome::Loaded { plan },
                Err(err) => {
                    log_err(&runtime, format!("Update of '{name}' failed ({err}); loading the old version back"));
                    if let Ok(old) = Module::from_bytes(runtime.clone(), &old_wasm).await {
                        if let Err(e) = Runtime::load_module(runtime.clone(), old).await {
                            log_err(&runtime, format!("Reloading the old '{name}' failed: {e}"));
                        }
                    }
                    match err {
                        IntersticeError::MigrationRequired(plan) => LoadOutcome::NeedsDecision { plan },
                        err => LoadOutcome::Failed(err.to_string()),
                    }
                }
            };
        }
        match Runtime::load_module_with(runtime.clone(), module, policy).await {
            Ok((_, plan)) => LoadOutcome::Loaded { plan },
            Err(IntersticeError::MigrationRequired(plan)) => {
                runtime.logger.log(
                    &format!("Load of '{name}' refused, nothing changed.\n{plan}"),
                    LogSource::Runtime,
                    LogLevel::Warning,
                );
                LoadOutcome::NeedsDecision { plan }
            }
            Err(err) => {
                log_err(&runtime, format!("Failed to load module '{}': {}", name, err));
                LoadOutcome::Failed(err.to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use interstice_abi::{ModuleSelection, NodeSelection, ReducerTableRef};

    fn table_ref(node: NodeSelection, module: ModuleSelection, table: &str) -> ReducerTableRef {
        ReducerTableRef {
            node_selection: node,
            module_selection: module,
            table_name: table.into(),
        }
    }

    #[test]
    fn own_and_foreign_refs_to_one_table_share_a_key() {
        // `graphics` writing its own `Layer` and `desktop` reading
        // `graphics.layer` must conflict in the scheduler.
        let own = normalized_table_key(
            "graphics",
            &table_ref(NodeSelection::Current, ModuleSelection::Current, "Layer"),
        );
        let foreign = normalized_table_key(
            "desktop",
            &table_ref(
                NodeSelection::Current,
                ModuleSelection::Other("Graphics".into()),
                "layer",
            ),
        );
        assert_eq!(own, foreign);
        // Same table name in two modules: distinct keys.
        let other = normalized_table_key(
            "desktop",
            &table_ref(NodeSelection::Current, ModuleSelection::Current, "layer"),
        );
        assert_ne!(own, other);
    }
}
