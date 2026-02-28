use crate::{
    app::App,
    audio::AudioEngine,
    error::IntersticeError,
    logger::{LogLevel, LogSource, Logger},
    network::{Network, NetworkHandle},
    persistence::{PeerTokenStore, TableStore},
    runtime::{Runtime, event::EventInstance, module::Module},
};
use interstice_abi::{ModuleSchema, NodeSchema};
use std::sync::Arc;
use std::{collections::HashSet, fs::File, path::Path, sync::Mutex, thread};
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
        std::fs::create_dir_all(&data_path).expect("Should be able to create node data path");
        std::fs::create_dir_all(&modules_path).expect("Should be able to create modules path");
        let table_store = TableStore::new(Some(modules_path.clone()));

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
        let audio_state = Arc::new(Mutex::new(
            crate::runtime::host_calls::audio::AudioState::new(
                crate::runtime::host_calls::audio::start_audio_thread(),
            ),
        ));
        let run_app_notify = Arc::new(Notify::new());
        let runtime = Arc::new(Runtime::new(
            id,
            Some(modules_path),
            table_store,
            event_sender.clone(),
            network_handle.clone(),
            audio_state,
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
        let modules_path = data_path.join("modules");
        let table_store = TableStore::new(Some(modules_path.clone()));

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
        let audio_state = Arc::new(Mutex::new(
            crate::runtime::host_calls::audio::AudioState::new(
                crate::runtime::host_calls::audio::start_audio_thread(),
            ),
        ));
        let run_app_notify = Arc::new(Notify::new());
        let runtime = Arc::new(Runtime::new(
            id,
            Some(modules_path.clone()),
            table_store,
            event_sender.clone(),
            network_handle.clone(),
            audio_state,
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

        // Load all modules in dependency order
        let mut modules_to_load = Vec::new();
        for entry in std::fs::read_dir(&modules_path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            std::fs::create_dir_all(path.join("logs")).unwrap();
            std::fs::create_dir_all(path.join("snapshots")).unwrap();

            let wasm_path = path.join("module.wasm");
            if !wasm_path.exists() {
                continue;
            }

            let module = Module::from_file(runtime.clone(), &wasm_path).await?;
            modules_to_load.push((module.schema.name.clone(), module));
        }

        // Topologically sort modules so dependencies are loaded first
        let mut remaining = modules_to_load;
        let mut loaded = HashSet::new();
        let all_names: HashSet<_> = remaining.iter().map(|(name, _)| name.clone()).collect();
        while !remaining.is_empty() {
            let mut progressed = false;
            let mut next_remaining = Vec::new();

            for (name, module) in remaining.into_iter() {
                let deps = module
                    .schema
                    .module_dependencies
                    .iter()
                    .map(|d| d.module_name.clone())
                    .collect::<Vec<_>>();

                // Fail fast if a declared dependency is missing entirely from disk
                if let Some(missing) = deps.iter().find(|d| !all_names.contains(*d)) {
                    return Err(IntersticeError::ModuleNotFound(
                        missing.clone(),
                        format!("Required by '{}' on node startup", name),
                    ));
                }

                if deps.iter().all(|d| loaded.contains(d)) {
                    Runtime::load_module(runtime.clone(), module, false).await?;
                    loaded.insert(name);
                    progressed = true;
                } else {
                    next_remaining.push((name, module));
                }
            }

            if !progressed {
                return Err(IntersticeError::Internal(
                    "Module dependency cycle detected while loading node modules".into(),
                ));
            }

            remaining = next_remaining;
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
                let audio_engine = AudioEngine::new(
                    runtime.audio_state.clone(),
                    runtime.authority_modules.clone(),
                    runtime.event_sender.clone(),
                );
                audio_engine.spawn();
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
        self.runtime.persistence.clear_all()?;
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
