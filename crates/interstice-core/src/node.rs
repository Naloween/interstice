use crate::{
    app::App,
    error::IntersticeError,
    logger::{LogLevel, LogSource, Logger},
    network::{Network, NetworkHandle},
    persistence::PeerTokenStore,
    persistence::TransactionLog,
    runtime::{Runtime, event::EventInstance, module::Module},
};
use interstice_abi::{ModuleSchema, NodeSchema};
use std::sync::Arc;
use std::{fs::File, path::Path, sync::Mutex, thread};
use tokio::sync::{
    Notify,
    mpsc::{self, UnboundedReceiver},
};
use uuid::Uuid;

pub type NodeId = Uuid;

pub struct Node {
    pub id: NodeId,
    pub(crate) network_handle: NetworkHandle,
    run_app_notify: Arc<Notify>,
    event_receiver: UnboundedReceiver<EventInstance>,
    network: Network,
    app: App,
    runtime: Arc<Runtime>,
    logger: Logger,
}

impl Node {
    pub fn new(nodes_path: &Path, port: u32) -> Result<Self, IntersticeError> {
        let id = Uuid::new_v4();
        let data_path = nodes_path.join(id.to_string());
        let modules_path = data_path.join("modules");
        let transaction_log_path = data_path.join("transaction_log");
        std::fs::create_dir_all(&data_path).expect("Should be able to create node data path");
        std::fs::create_dir_all(&modules_path).expect("Should be able to create modules path");

        let address = format!("127.0.0.1:{}", port);

        let (event_sender, event_receiver) = mpsc::unbounded_channel();

        let logger = Logger::new(
            File::create(data_path.join("node.log")).expect("Should be able to create log file"),
        );

        let peer_tokens = Arc::new(Mutex::new(PeerTokenStore::load_or_create(
            data_path.join("peer_tokens.toml"),
        )?));
        let network = Network::new(
            id,
            address.clone(),
            event_sender.clone(),
            peer_tokens,
            logger.clone(),
        );
        let network_handle = network.get_handle();
        let gpu = Arc::new(Mutex::new(None));
        let run_app_notify = Arc::new(Notify::new());
        let runtime = Arc::new(Runtime::new(
            Some(modules_path),
            TransactionLog::new(&transaction_log_path)?,
            event_sender.clone(),
            network_handle.clone(),
            gpu,
            run_app_notify.clone(),
            logger.clone(),
        )?);
        let gpu_call_receiver = runtime.take_gpu_call_receiver();
        let app = App::new(
            id,
            event_sender.clone(),
            runtime.gpu.clone(),
            runtime.clone(),
            gpu_call_receiver,
        );

        let node = Self {
            id,
            runtime,
            app,
            network,
            network_handle,
            event_receiver,
            run_app_notify,
            logger,
        };

        Ok(node)
    }

    pub async fn load(nodes_path: &Path, id: NodeId, port: u32) -> Result<Self, IntersticeError> {
        let data_path = nodes_path.join(id.to_string());
        let address = format!("127.0.0.1:{}", port);
        let transaction_log_path = data_path.join("transaction_log");
        let modules_path = data_path.join("modules");

        let (event_sender, event_receiver) = mpsc::unbounded_channel();

        // Open log file to append new logs on.
        let logger_file = File::options()
            .append(true)
            .open(data_path.join("node.log"))
            .expect("Should be able to open log file");
        let logger = Logger::new(logger_file);

        let peer_tokens = Arc::new(Mutex::new(PeerTokenStore::load_or_create(
            data_path.join("peer_tokens.toml"),
        )?));
        let network = Network::new(
            id,
            address.clone(),
            event_sender.clone(),
            peer_tokens,
            logger.clone(),
        );
        let network_handle = network.get_handle();
        let gpu = Arc::new(Mutex::new(None));
        let run_app_notify = Arc::new(Notify::new());
        let runtime = Arc::new(Runtime::new(
            Some(modules_path.clone()),
            TransactionLog::new(&transaction_log_path)?,
            event_sender.clone(),
            network_handle.clone(),
            gpu,
            run_app_notify.clone(),
            logger.clone(),
        )?);
        let gpu_call_receiver = runtime.take_gpu_call_receiver();
        let app = App::new(
            id,
            event_sender.clone(),
            runtime.gpu.clone(),
            runtime.clone(),
            gpu_call_receiver,
        );

        // Load all modules
        for module_path in std::fs::read_dir(&modules_path).unwrap() {
            let module_path = module_path.unwrap().path();
            if module_path.extension().and_then(|s| s.to_str()) == Some("wasm") {
                let module = Module::from_file(runtime.clone(), &module_path).await?;
                Runtime::load_module(runtime.clone(), module, false).await?;
            }
        }

        // Replay transaction logs to restore state once all modules are available
        runtime.replay()?;

        let node = Self {
            id,
            runtime,
            app,
            network,
            network_handle,
            event_receiver,
            run_app_notify,
            logger,
        };

        Ok(node)
    }

    pub fn new_elusive(port: u32) -> Result<Self, IntersticeError> {
        let id = Uuid::new_v4();
        let address = format!("127.0.0.1:{}", port);
        let (event_sender, event_receiver) = mpsc::unbounded_channel();
        let logger = Logger::new_stdout();

        let peer_tokens = Arc::new(Mutex::new(PeerTokenStore::new_in_memory()));
        let network = Network::new(
            id,
            address.clone(),
            event_sender.clone(),
            peer_tokens,
            logger.clone(),
        );
        let network_handle = network.get_handle();
        let gpu = Arc::new(Mutex::new(None));
        let run_app_notify = Arc::new(Notify::new());
        let runtime = Arc::new(Runtime::new(
            None,
            TransactionLog::new_in_memory(),
            event_sender.clone(),
            network_handle.clone(),
            gpu,
            run_app_notify.clone(),
            logger.clone(),
        )?);
        let gpu_call_receiver = runtime.take_gpu_call_receiver();
        let app = App::new(
            id,
            event_sender.clone(),
            runtime.gpu.clone(),
            runtime.clone(),
            gpu_call_receiver,
        );

        Ok(Self {
            id,
            runtime,
            app,
            network,
            network_handle,
            event_receiver,
            run_app_notify,
            logger,
        })
    }

    pub fn log(&self, message: &str, source: LogSource, level: LogLevel) {
        self.logger.log(message, source, level);
    }

    pub async fn start(self) -> Result<(), IntersticeError> {
        let Node {
            id,
            runtime,
            app,
            mut network,
            network_handle: _network_handle,
            event_receiver,
            run_app_notify,
            logger,
        } = self;

        logger.log(
            &format!("Starting node with ID: {}", id),
            LogSource::Node,
            LogLevel::Info,
        );

        // Run network events
        network.listen()?;
        let _net_handle = network.run();

        thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to build runtime");
            let local = tokio::task::LocalSet::new();
            local.block_on(&rt, async move {
                Runtime::run(runtime, event_receiver).await;
            });
        });

        run_app_notify.notified().await;
        app.run();

        Ok(())
    }

    pub async fn schema(&self, name: String) -> NodeSchema {
        NodeSchema {
            name,
            address: self.network_handle.address.clone(),
            modules: self
                .runtime
                .modules
                .lock()
                .unwrap()
                .values()
                .map(|m| (*m.schema).clone())
                .collect(),
        }
    }

    pub async fn clear_logs(&mut self) -> Result<(), IntersticeError> {
        self.runtime
            .transaction_logs
            .lock()
            .unwrap()
            .delete_all_logs()?;
        Ok(())
    }

    pub async fn load_module_from_file<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<ModuleSchema, IntersticeError> {
        let module = Module::from_file(self.runtime.clone(), path.as_ref()).await?;
        Runtime::load_module(self.runtime.clone(), module, true).await
    }

    pub async fn load_module_from_bytes(
        &self,
        bytes: &[u8],
    ) -> Result<ModuleSchema, IntersticeError> {
        let module = Module::from_bytes(self.runtime.clone(), bytes).await?;
        Runtime::load_module(self.runtime.clone(), module, true).await
    }

    pub fn get_port(&self) -> u32 {
        self.network_handle
            .address
            .split(':')
            .last()
            .unwrap()
            .parse()
            .unwrap()
    }
}
