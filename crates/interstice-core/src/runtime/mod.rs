mod authority;
pub mod event;
pub mod host_calls;
mod module;
mod reducer;
mod table;
pub mod transaction;
mod wasm;

use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};

use crate::{
    IntersticeError,
    network::NetworkHandle,
    node::NodeId,
    persistence::TransactionLog,
    runtime::{
        authority::AuthorityEntry,
        event::EventInstance,
        host_calls::gpu::GpuState,
        module::Module,
        reducer::ReducerFrame,
        wasm::{StoreState, linker::define_host_calls},
    },
};
use interstice_abi::{Authority, SubscriptionEventSchema};
use tokio::{
    sync::{
        Notify,
        mpsc::{UnboundedReceiver, UnboundedSender},
    },
    task::JoinHandle,
};
use wasmtime::{Engine, Linker};

pub struct Runtime {
    pub(crate) gpu: Arc<Mutex<Option<GpuState>>>,
    pub(crate) modules: Arc<Mutex<HashMap<String, Arc<Module>>>>,
    pub(crate) authority_modules: Arc<Mutex<HashMap<Authority, AuthorityEntry>>>,
    pub(crate) call_stack: Arc<Mutex<Vec<ReducerFrame>>>,
    pub(crate) engine: Arc<Engine>,
    pub(crate) linker: Arc<Mutex<Linker<StoreState>>>,
    pub(crate) event_sender: UnboundedSender<EventInstance>,
    pub(crate) network_handle: NetworkHandle,
    pub(crate) transaction_logs: Arc<Mutex<TransactionLog>>,
    pub(crate) app_initialized: Arc<Mutex<bool>>,
    pub(crate) pending_app_modules: Arc<Mutex<Vec<Module>>>,
    run_app_notify: Arc<Notify>,
    node_subscriptions: Arc<Mutex<HashMap<NodeId, Vec<SubscriptionEventSchema>>>>,
}

impl Runtime {
    pub fn new(
        transaction_log_path: &Path,
        event_sender: UnboundedSender<EventInstance>,
        network_handle: NetworkHandle,
        gpu: Arc<Mutex<Option<GpuState>>>,
        run_app_notify: Arc<Notify>,
    ) -> Result<Self, IntersticeError> {
        let engine = Arc::new(Engine::default());
        let mut linker = Linker::new(&engine);
        define_host_calls(&mut linker).map_err(|err| {
            IntersticeError::Internal(format!("Couldn't add host calls to the linker: {}", err))
        })?;
        Ok(Self {
            gpu,
            call_stack: Arc::new(Mutex::new(Vec::new())),
            engine,
            linker: Arc::new(Mutex::new(linker)),
            event_sender,
            network_handle,
            transaction_logs: Arc::new(Mutex::new(TransactionLog::new(transaction_log_path)?)),
            modules: Arc::new(Mutex::new(HashMap::new())),
            authority_modules: Arc::new(Mutex::new(HashMap::new())),
            app_initialized: Arc::new(Mutex::new(false)),
            pending_app_modules: Arc::new(Mutex::new(Vec::new())),
            run_app_notify,
            node_subscriptions: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub fn run(
        runtime: Arc<Runtime>,
        mut event_receiver: UnboundedReceiver<EventInstance>,
    ) -> JoinHandle<()> {
        return tokio::spawn(async move {
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
                            if let Err(err) = Runtime::publish_module(runtime.clone(), module).await
                            {
                                eprintln!("Failed to load module '{}': {}", module_name, err);
                            }
                        }
                        *runtime.app_initialized.lock().unwrap() = true;
                    }
                    EventInstance::RemoteReducerCall {
                        module_name,
                        reducer_name,
                        input,
                    } => {
                        // Invoke the requested reducer with no args (network
                        // reducer packet currently does not carry args).
                        let _ = runtime.call_reducer(&module_name, &reducer_name, input);
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
                    event => {
                        let triggered = runtime.find_subscriptions(&event).unwrap();

                        for sub in triggered {
                            runtime.invoke_subscription(sub, event.clone()).unwrap();
                        }
                    }
                }
            }
        });
    }

    pub fn replay(&self) -> Result<(), IntersticeError> {
        let transactions = self.transaction_logs.lock().unwrap().read_all()?;

        for transaction in transactions {
            let _events = self.apply_transaction(transaction)?;
        }

        Ok(())
    }
}
