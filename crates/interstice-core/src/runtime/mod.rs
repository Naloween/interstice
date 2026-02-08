mod authority;
pub mod event;
pub mod host_calls;
pub mod module;
mod query;
mod reducer;
mod table;
pub mod transaction;
mod wasm;

use crate::{
    IntersticeError,
    logger::{LogLevel, LogSource, Logger},
    network::NetworkHandle,
    node::NodeId,
    persistence::TransactionLog,
    runtime::{
        authority::AuthorityEntry,
        event::EventInstance,
        host_calls::gpu::GpuState,
        module::Module,
        reducer::CallFrame,
        wasm::{StoreState, linker::define_host_calls},
    },
};
use interstice_abi::{Authority, IntersticeValue, ModuleEvent, SubscriptionEventSchema};
use notify::RecommendedWatcher;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
};
use tokio::sync::{
    Notify,
    mpsc::{UnboundedReceiver, UnboundedSender},
    oneshot,
};
use wasmtime::{Config, Engine, Linker};

pub struct Runtime {
    pub(crate) gpu: Arc<Mutex<Option<GpuState>>>,
    pub(crate) modules: Arc<Mutex<HashMap<String, Arc<Module>>>>,
    pub(crate) authority_modules: Arc<Mutex<HashMap<Authority, AuthorityEntry>>>,
    pub(crate) call_stack: Arc<Mutex<Vec<CallFrame>>>,
    pub(crate) engine: Arc<Engine>,
    pub(crate) linker: Arc<Linker<StoreState>>,
    pub(crate) event_sender: UnboundedSender<EventInstance>,
    pub(crate) network_handle: NetworkHandle,
    pub(crate) transaction_logs: Arc<Mutex<TransactionLog>>,
    pub(crate) app_initialized: Arc<Mutex<bool>>,
    pub(crate) pending_app_modules: Arc<Mutex<Vec<Module>>>,
    pub(crate) pending_query_responses:
        Arc<Mutex<HashMap<String, oneshot::Sender<IntersticeValue>>>>,
    pub(crate) reducer_sender: UnboundedSender<ReducerJob>,
    reducer_receiver: Mutex<Option<UnboundedReceiver<ReducerJob>>>,
    pub(crate) gpu_call_sender: mpsc::Sender<GpuCallRequest>,
    gpu_call_receiver: Mutex<Option<mpsc::Receiver<GpuCallRequest>>>,
    pub(crate) modules_path: PathBuf,
    run_app_notify: Arc<Notify>,
    node_subscriptions: Arc<Mutex<HashMap<NodeId, Vec<SubscriptionEventSchema>>>>,
    pub(crate) logger: Logger,
    pub(crate) file_watchers: Arc<Mutex<Vec<RecommendedWatcher>>>,
}

#[derive(Debug, Clone)]
pub struct ReducerJob {
    pub module_name: String,
    pub reducer_name: String,
    pub input: IntersticeValue,
    pub completion: Option<mpsc::Sender<()>>,
}

#[derive(Debug, Clone)]
pub enum GpuCallResult {
    None,
    I64(i64),
    TextureFormat(interstice_abi::TextureFormat),
}

pub struct GpuCallRequest {
    pub call: interstice_abi::GpuCall,
    pub respond_to: oneshot::Sender<Result<GpuCallResult, IntersticeError>>,
}

struct CompletionGuard(Option<mpsc::Sender<()>>);

impl CompletionGuard {
    fn new(sender: mpsc::Sender<()>) -> Self {
        Self(Some(sender))
    }
}

impl Drop for CompletionGuard {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

impl Runtime {
    pub fn new(
        modules_path: PathBuf,
        transaction_log_path: &Path,
        event_sender: UnboundedSender<EventInstance>,
        network_handle: NetworkHandle,
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
            gpu,
            call_stack: Arc::new(Mutex::new(Vec::new())),
            engine,
            linker: Arc::new(linker),
            event_sender,
            network_handle,
            transaction_logs: Arc::new(Mutex::new(TransactionLog::new(transaction_log_path)?)),
            modules_path,
            modules: Arc::new(Mutex::new(HashMap::new())),
            authority_modules: Arc::new(Mutex::new(HashMap::new())),
            app_initialized: Arc::new(Mutex::new(false)),
            pending_app_modules: Arc::new(Mutex::new(Vec::new())),
            pending_query_responses: Arc::new(Mutex::new(HashMap::new())),
            reducer_sender,
            reducer_receiver: Mutex::new(Some(reducer_receiver)),
            gpu_call_sender,
            gpu_call_receiver: Mutex::new(Some(gpu_call_receiver)),
            run_app_notify,
            node_subscriptions: Arc::new(Mutex::new(HashMap::new())),
            logger,
            file_watchers: Arc::new(Mutex::new(Vec::new())),
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
                    .call_reducer(&job.module_name, &job.reducer_name, job.input)
                    .await;
            }
        });

        while let Some(event) = event_receiver.recv().await {
            match event {
                EventInstance::AppInitialized => {
                    let modules = runtime
                        .pending_app_modules
                        .lock()
                        .unwrap()
                        .drain(..)
                        .collect::<Vec<_>>();
                    for module in modules {
                        let module_name = module.schema.name.clone();
                        if let Err(err) = Runtime::publish_module(runtime.clone(), module).await {
                            runtime.logger.log(
                                &format!("Failed to load module '{}': {}", module_name, err),
                                LogSource::Runtime,
                                LogLevel::Error,
                            );
                        }
                    }
                    *runtime.app_initialized.lock().unwrap() = true;
                }
                EventInstance::RemoteReducerCall {
                    module_name,
                    reducer_name,
                    input,
                } => {
                    let _ = runtime.reducer_sender.send(ReducerJob {
                        module_name,
                        reducer_name,
                        input,
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
                        let result = runtime
                            .call_query(&module_name, &query_name, input)
                            .await
                            .unwrap_or(IntersticeValue::Void);
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
                EventInstance::RequestSubscription {
                    requesting_node_id,
                    event,
                } => {
                    runtime
                        .node_subscriptions
                        .lock()
                        .unwrap()
                        .entry(requesting_node_id)
                        .or_insert(Vec::new())
                        .push(event);
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
                        let module_name = Module::from_bytes(runtime.clone(), &wasm_binary)
                            .await
                            .map(|m| m.schema.name.clone())
                            .unwrap_or_else(|_| "unknown".into());
                        let _ = runtime.event_sender.send(EventInstance::Module(
                            ModuleEvent::PublishRequest {
                                node_id: source_node_id.to_string(),
                                module_name,
                                wasm_binary,
                            },
                        ));
                    } else {
                        Runtime::load_module(
                            runtime.clone(),
                            Module::from_bytes(runtime.clone(), &wasm_binary)
                                .await
                                .unwrap(),
                        )
                        .await
                        .unwrap();
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
                event => {
                    let triggered = runtime.find_subscriptions(&event).unwrap();

                    for sub in triggered {
                        let _ = runtime.invoke_subscription(sub, event.clone());
                    }
                }
            }
        }
    }

    pub fn replay(&self) -> Result<(), IntersticeError> {
        let transactions = self.transaction_logs.lock().unwrap().read_all()?;

        for transaction in transactions {
            let _events = self.apply_transaction(transaction)?;
        }

        Ok(())
    }
}
