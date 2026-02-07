use crate::{
    app::App,
    error::IntersticeError,
    logger::{LogLevel, LogSource, Logger},
    network::{Network, NetworkHandle},
    runtime::{Runtime, event::EventInstance, module::Module},
};
use interstice_abi::{ModuleSchema, NodeSchema};
use std::sync::Arc;
use std::{fs::File, path::Path, sync::Mutex};
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
    pub fn new(root_data_path: &Path, port: u32) -> Result<Self, IntersticeError> {
        let id = Uuid::new_v4();
        let data_path = root_data_path.join(id.to_string());
        std::fs::create_dir_all(&data_path).expect("Should be able to create node data path");
        let modules_path = data_path.join("modules");
        std::fs::create_dir_all(&modules_path).expect("Should be able to create modules path");

        let address = format!("127.0.0.1:{}", port);
        let transaction_log_path = data_path.join("transaction_log");

        let (event_sender, event_receiver) = mpsc::unbounded_channel();

        let logger = Logger::new(
            File::create(data_path.join("node.log")).expect("Should be able to create log file"),
        );

        let network = Network::new(id, address.clone(), event_sender.clone(), logger.clone());
        let network_handle = network.get_handle();
        let gpu = Arc::new(Mutex::new(None));
        let app = App::new(id, event_sender.clone(), gpu.clone());
        let run_app_notify = Arc::new(Notify::new());
        let runtime = Arc::new(Runtime::new(
            modules_path,
            &transaction_log_path,
            event_sender.clone(),
            network_handle.clone(),
            gpu,
            run_app_notify.clone(),
            logger.clone(),
        )?);

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

    pub async fn load(
        root_data_path: &Path,
        id: NodeId,
        port: u32,
    ) -> Result<Self, IntersticeError> {
        let data_path = root_data_path.join(id.to_string());
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

        let network = Network::new(id, address.clone(), event_sender.clone(), logger.clone());
        let network_handle = network.get_handle();
        let gpu = Arc::new(Mutex::new(None));
        let app = App::new(id, event_sender.clone(), gpu.clone());
        let run_app_notify = Arc::new(Notify::new());
        let runtime = Arc::new(Runtime::new(
            modules_path.clone(),
            &transaction_log_path,
            event_sender.clone(),
            network_handle.clone(),
            gpu,
            run_app_notify.clone(),
            logger.clone(),
        )?);

        // Load all modules
        for module_path in std::fs::read_dir(&modules_path).unwrap() {
            let module_path = module_path.unwrap().path();
            if module_path.extension().and_then(|s| s.to_str()) == Some("wasm") {
                let module = Module::from_file(runtime.clone(), &module_path)?;
                Runtime::load_module(runtime.clone(), module).await?;
            }
        }

        // Replay transaction logs to restore state
        runtime.replay()?; // Doesn't work when module loaded afetr app initialization, need to load modules before replaying logs

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

    pub fn log(&self, message: &str, source: LogSource, level: LogLevel) {
        self.logger.log(message, source, level);
    }

    pub async fn start(mut self) -> Result<(), IntersticeError> {
        self.logger.log(
            &format!("Starting node with ID: {}", self.id),
            LogSource::Node,
            LogLevel::Info,
        );

        // Run network events
        self.network.listen()?;
        let _net_handle = self.network.run();
        let _runtime_handle = Runtime::run(self.runtime, self.event_receiver);

        self.run_app_notify.notified().await;
        self.app.run();

        // let _ = tokio::join!(net_handle, runtime_handle);

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
        let module = Module::from_file(self.runtime.clone(), path.as_ref())?;
        Runtime::load_module(self.runtime.clone(), module).await
    }

    pub async fn load_module_from_bytes(
        &self,
        bytes: &[u8],
    ) -> Result<ModuleSchema, IntersticeError> {
        let module = Module::from_bytes(self.runtime.clone(), bytes)?;
        Runtime::load_module(self.runtime.clone(), module).await
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
