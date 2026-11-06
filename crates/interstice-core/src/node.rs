use crate::{
    MigrationPolicy,
    security::NodeConfig,
    app::App,
    bundle::{Bundle, BundleManifest},
    audio::AudioEngine,
    error::IntersticeError,
    logger::{LogLevel, LogSource, Logger},
    network::{Network, NetworkHandle},
    network::identity::{KnownPeers, LocalIdentity, Trust},
    persistence::TableStore,
    runtime::{
        Runtime,
        event::EventInstance,
        module::Module,
        reducer::{CompletionToken, Job},
    },
};
use crossbeam_channel;
use interstice_abi::{ModuleSchema, NodeSchema};
use parking_lot::Mutex;
use std::sync::Arc;
use std::{collections::HashSet, fs::File, path::Path, thread};
use tokio::sync::{
    Notify,
    mpsc::{self, UnboundedReceiver},
};
use uuid::Uuid;

pub type NodeId = Uuid;

/// The node's key pair, and the keys remembered for legacy peers (§7.4).
pub const IDENTITY_FILE: &str = "identity.key";
pub const KNOWN_PEERS_FILE: &str = "known_peers.toml";

/// Answers a migration plan that asks: `(module, rendered plan) -> policy`;
/// `Safe` means abort (keep the old version). DESIGN §3.3, §9.1.
pub type Decide = Box<dyn Fn(&str, &str) -> MigrationPolicy + Send + Sync>;

/// Bounded reducer queue capacity. When full, producers **block** (see
/// `send` on the ingress channel) so TCP / event paths apply backpressure
/// instead of silently dropping work (`try_send` must not be used here).
const REDUCER_QUEUE_CAPACITY: usize = 8_192;

pub struct Node {
    pub id: NodeId,
    pub(crate) network_handle: NetworkHandle,
    run_app_notify: Arc<Notify>,
    event_receiver: UnboundedReceiver<(EventInstance, Option<CompletionToken>)>,
    network: Network,
    app: App,
    runtime: Arc<Runtime>,
    logger: Logger,
    data_path: std::path::PathBuf,
    trust: Arc<Trust>,
}

impl Node {
    /// Create a new node.
    ///
    /// # Arguments
    /// * `nodes_path` - Directory to store node data
    /// * `port` - Port to listen on (binds to 0.0.0.0:{port})
    /// * `public_address` - Address to advertise for external connections (e.g., "127.0.0.1:{port}" for local, "hostname:{port}" for external)
    pub fn new(
        nodes_path: &Path,
        port: u32,
        public_address: String,
    ) -> Result<Self, IntersticeError> {
        // The id is the key's (DESIGN §7.4).
        let (identity, key_pem) = LocalIdentity::generate()?;
        let id = identity.id;
        let data_path = nodes_path.join(id.to_string());
        let modules_path = data_path.join("modules");
        std::fs::create_dir_all(&data_path).expect("Should be able to create node data path");
        std::fs::create_dir_all(&modules_path).expect("Should be able to create modules path");
        let table_store = TableStore::new(Some(modules_path.clone()));

        let bind_address = format!("0.0.0.0:{}", port);

        let (event_sender, event_receiver) = mpsc::unbounded_channel();

        let logger = Logger::new(
            File::create(data_path.join("node.log")).expect("Should be able to create log file"),
        );

        crate::network::identity::save_key(&data_path.join(IDENTITY_FILE), &key_pem)?;
        let trust = Arc::new(Trust {
            identity,
            known: KnownPeers::load(Some(data_path.join(KNOWN_PEERS_FILE))),
        });

        // Bounded reducer ingress: producers use blocking `send` so a full queue
        // stalls readers / the event loop instead of dropping reducer jobs.
        let (reducer_sender, reducer_receiver) =
            crossbeam_channel::bounded::<Job>(REDUCER_QUEUE_CAPACITY);

        let network = Network::new(
            id,
            bind_address.clone(),
            public_address.clone(),
            event_sender.clone(),
            reducer_sender.clone(),
            trust.clone(),
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
            reducer_sender,
            reducer_receiver,
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
            data_path,
            trust,
        };

        Ok(node)
    }

    pub async fn load(
        nodes_path: &Path,
        id: NodeId,
        port: u32,
        public_address: String,
    ) -> Result<Self, IntersticeError> {
        let data_path = nodes_path.join(id.to_string());
        let bind_address = format!("0.0.0.0:{}", port);
        let modules_path = data_path.join("modules");
        let table_store = TableStore::new(Some(modules_path.clone()));

        let (event_sender, event_receiver) = mpsc::unbounded_channel();

        // Open log file to append new logs on.
        let logger_file = File::options()
            .append(true)
            .open(data_path.join("node.log"))
            .expect("Should be able to open log file");
        let logger = Logger::new(logger_file);

        // Nodes created before keys keep their id and get a key now.
        let trust = Arc::new(Trust {
            identity: LocalIdentity::load_or_create(&data_path.join(IDENTITY_FILE), Some(id))?,
            known: KnownPeers::load(Some(data_path.join(KNOWN_PEERS_FILE))),
        });

        let (reducer_sender, reducer_receiver) =
            crossbeam_channel::bounded::<Job>(REDUCER_QUEUE_CAPACITY);

        let network = Network::new(
            id,
            bind_address.clone(),
            public_address.clone(),
            event_sender.clone(),
            reducer_sender.clone(),
            trust.clone(),
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
            reducer_sender,
            reducer_receiver,
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
            data_path,
            trust,
        };

        Ok(node)
    }

    /// This node's `node.toml` (DESIGN §7.4).
    pub fn config(&self) -> Result<NodeConfig, IntersticeError> {
        NodeConfig::load(&self.data_path)
    }

    pub fn save_config(&self, config: &NodeConfig) -> Result<(), IntersticeError> {
        config.save(&self.data_path)
    }

    /// Make `id` an owner of this node (DESIGN §7.4). Ids are bound to
    /// keys, so no secret is needed.
    pub fn add_owner(&self, id: &str) -> Result<(), IntersticeError> {
        let mut config = self.config()?;
        if config.add_owner(id) {
            self.save_config(&config)?;
        }
        Ok(())
    }

    pub fn log(&self, message: &str, source: LogSource, level: LogLevel) {
        self.logger.log(message, source, level);
    }

    pub async fn start(self, initial_modules: &[&[u8]]) -> Result<(), IntersticeError> {
        let given = initial_modules.iter().map(|b| b.to_vec()).collect();
        self.start_inner(given, None, None).await
    }

    /// Install or update `bundle` on this node, then run it (DESIGN §9.1).
    /// `decide(module, plan)` answers a migration plan that asks; returning
    /// `Safe` keeps the module's old version. Without `decide` such a plan
    /// fails the start.
    pub async fn start_bundle(self, bundle: Bundle, decide: Option<Decide>) -> Result<(), IntersticeError> {
        if let Some(modules_path) = self.runtime.modules_path.clone() {
            // Seed data only for modules this node does not have yet.
            for module in &bundle.modules {
                let (Some(name), false) = (&module.name, module.data.is_empty()) else { continue };
                let dir = modules_path.join(name);
                if dir.join("state.toml").exists() {
                    continue;
                }
                crate::bundle::write_seed(&dir, &module.data)?;
                std::fs::write(dir.join("module.wasm"), &module.wasm)
                    .map_err(|e| IntersticeError::Internal(format!("seed {name}: {e}")))?;
            }
        }
        let given = bundle.modules.into_iter().map(|m| m.wasm).collect();
        self.start_inner(given, decide, Some(bundle.manifest)).await
    }

    async fn start_inner(
        self,
        given: Vec<Vec<u8>>,
        decide: Option<Decide>,
        bundle: Option<BundleManifest>,
    ) -> Result<(), IntersticeError> {
        let (app, run_app_notify, _handle) = self.boot(given, decide, bundle).await?;
        run_app_notify.notified().await;
        app.run();
        Ok(())
    }

    /// Start the node without a window and return once `modules` are loaded
    /// (DESIGN §10, Embedding). The node keeps running on its own threads;
    /// the handle calls its reducers and queries in process, as the kernel.
    /// A module asking for the GPU or input authority gets no window.
    pub async fn start_headless(self, modules: &[&[u8]]) -> Result<NodeHandle, IntersticeError> {
        let given = modules.iter().map(|b| b.to_vec()).collect();
        let (_app, _notify, handle) = self.boot(given, None, None).await?;
        Ok(handle)
    }

    /// Everything but the window: config, network, runtime loop, modules.
    async fn boot(
        self,
        given: Vec<Vec<u8>>,
        decide: Option<Decide>,
        bundle: Option<BundleManifest>,
    ) -> Result<(App, Arc<Notify>, NodeHandle), IntersticeError> {
        let Node {
            id,
            runtime,
            app,
            mut network,
            network_handle: _network_handle,
            event_receiver,
            run_app_notify,
            logger,
            data_path,
            trust: _trust,
        } = self;

        // Security config (DESIGN §7.4): owners, file root, listen host.
        let config = NodeConfig::load(&data_path)?;
        runtime.security.apply(&config, &data_path);
        network.set_bind_host(config.listen_host());

        logger.log(
            &format!("Starting node with ID: {}", id),
            LogSource::Node,
            LogLevel::Info,
        );
        // Phase breakdown (DESIGN §6.8): printed every 10 s while on.
        if crate::runtime::profile::init_from_env() {
            thread::spawn(|| loop {
                thread::sleep(std::time::Duration::from_secs(10));
                eprintln!("{}", crate::runtime::profile::report());
            });
        }

        // Run network events
        network.listen().await?;
        let _net_handle = network.run();

        let runtime_for_thread = runtime.clone();

        thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to build runtime");
            let local = tokio::task::LocalSet::new();
            local.block_on(&rt, async move {
                let audio_engine = AudioEngine::new(
                    runtime_for_thread.audio_state.clone(),
                    runtime_for_thread.authority_modules.clone(),
                    runtime_for_thread.event_sender.clone(),
                );
                audio_engine.spawn();
                Runtime::run(runtime_for_thread, event_receiver).await;
            });
        });

        // Load modules only after the runtime loop is running, so async schema
        // responses required during module loading can be processed. The
        // persisted modules and the given ones (an example's, a bundle's)
        // load together in dependency order; a given wasm replaces the
        // persisted one through the migration plan (DESIGN §3.3, §9.1).
        let node_dir = runtime.modules_path.as_ref().and_then(|p| p.parent()).map(|p| p.to_path_buf());
        let previous = match (&bundle, &node_dir) {
            (Some(_), Some(dir)) => crate::bundle::read_record(dir),
            _ => None,
        };
        let previous_modules = previous.as_ref().map(|r| r.modules.clone()).unwrap_or_default();
        let (names, kept_old) =
            Node::load_modules(runtime.clone(), given, decide.as_ref(), &previous_modules).await?;
        if let (Some(manifest), Some(dir)) = (bundle, node_dir) {
            // A module that kept its old version means the node is not at
            // the new version yet: keep the old version in the record.
            let version = match (&previous, kept_old.is_empty()) {
                (Some(previous), false) => {
                    logger.log(
                        &format!(
                            "Bundle '{}' {} not fully applied (old versions kept: {}); the node stays at {}",
                            manifest.name,
                            manifest.version,
                            kept_old.join(", "),
                            previous.version
                        ),
                        LogSource::Node,
                        LogLevel::Warning,
                    );
                    previous.version.clone()
                }
                _ => manifest.version,
            };
            crate::bundle::write_record(
                &dir,
                &crate::bundle::InstallRecord { name: manifest.name, version, modules: names },
            )?;
        }

        let handle = NodeHandle { id, runtime };
        Ok((app, run_app_notify, handle))
    }

    pub async fn schema(&self, name: String) -> NodeSchema {
        NodeSchema {
            name,
            address: self.network_handle.address.clone(),
            modules: self
                .runtime
                .modules
                .lock()
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
        Runtime::load_module(self.runtime.clone(), module).await
    }

    pub async fn load_module_from_bytes(
        &self,
        bytes: &[u8],
    ) -> Result<ModuleSchema, IntersticeError> {
        let module = Module::from_bytes(self.runtime.clone(), bytes).await?;
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

    /// Load the persisted modules and `given` in dependency order; returns
    /// the names of the given modules. A given module replaces the persisted
    /// one of the same name. Persisted modules named in `previous` (the last
    /// bundle install) but not given are unloaded, their data kept.
    async fn load_modules(
        runtime: Arc<Runtime>,
        given: Vec<Vec<u8>>,
        decide: Option<&Decide>,
        previous: &[String],
    ) -> Result<(Vec<String>, Vec<String>), IntersticeError> {
        let mut kept_old = Vec::new();
        let mut candidates: Vec<(String, Module, Option<Vec<u8>>)> = Vec::new();
        let mut given_names = Vec::new();
        for bytes in given {
            let module = Module::from_bytes(runtime.clone(), &bytes).await?;
            let name = module.schema.name.clone();
            given_names.push(name.clone());
            candidates.push((name, module, Some(bytes)));
        }

        if let Some(modules_path) = runtime.modules_path.clone() {
            let mut dirs: Vec<_> = std::fs::read_dir(&modules_path)
                .map_err(|e| IntersticeError::Internal(format!("{}: {e}", modules_path.display())))?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.is_dir())
                .collect();
            dirs.sort();
            for path in dirs {
                std::fs::create_dir_all(path.join("logs")).ok();
                std::fs::create_dir_all(path.join("snapshots")).ok();
                let wasm_path = path.join("module.wasm");
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                if !wasm_path.exists() || given_names.contains(&name) {
                    continue;
                }
                if previous.contains(&name) {
                    // The bundle dropped it: unload, keep the data.
                    crate::runtime::module::write_module_state(&path, false);
                    runtime.logger.log(
                        &format!("Module '{name}' is no longer in the bundle: unloaded, its data kept"),
                        LogSource::Node,
                        LogLevel::Info,
                    );
                    continue;
                }
                // Skip modules explicitly marked as unloaded — their data stays on
                // disk and they can be loaded again later.
                if !crate::runtime::module::module_should_load(&path) {
                    continue;
                }
                let module = Module::from_file(runtime.clone(), &wasm_path).await?;
                candidates.push((module.schema.name.clone(), module, None));
            }
        }

        let mut remaining = candidates;
        let mut loaded = HashSet::new();
        let all_names: HashSet<_> = remaining.iter().map(|(name, _, _)| name.clone()).collect();
        while !remaining.is_empty() {
            let mut progressed = false;
            let mut next_remaining = Vec::new();

            for (name, module, bytes) in remaining.into_iter() {
                let deps = module
                    .schema
                    .module_dependencies
                    .iter()
                    .map(|d| d.module_name.clone())
                    .collect::<Vec<_>>();

                if let Some(missing) = deps.iter().find(|d| !all_names.contains(*d)) {
                    return Err(IntersticeError::ModuleNotFound(
                        missing.clone(),
                        format!("Required by '{}' on node startup", name),
                    ));
                }

                if deps.iter().all(|d| loaded.contains(d)) {
                    match bytes {
                        None => {
                            Runtime::load_module(runtime.clone(), module).await?;
                        }
                        Some(bytes) => {
                            if !Node::load_given(&runtime, &name, module, bytes, decide).await? {
                                kept_old.push(name.clone());
                            }
                        }
                    }
                    loaded.insert(name);
                    progressed = true;
                } else {
                    next_remaining.push((name, module, bytes));
                }
            }

            if !progressed {
                return Err(IntersticeError::Internal(
                    "Module dependency cycle detected while loading node modules".into(),
                ));
            }

            remaining = next_remaining;
        }

        // No separate replay() pass: `load_module` restores each module's tables
        // from disk in-load (it detects a reload), so state is already rebuilt.

        Ok((given_names, kept_old))
    }

    /// Load a given module over the node's data, asking `decide` when the
    /// migration plan needs a decision. `Ok(false)`: the old version was
    /// kept. The runtime logs the plan it applies.
    async fn load_given(
        runtime: &Arc<Runtime>,
        name: &str,
        module: Module,
        bytes: Vec<u8>,
        decide: Option<&Decide>,
    ) -> Result<bool, IntersticeError> {
        match Runtime::load_module_with(runtime.clone(), module, MigrationPolicy::Safe).await {
            Ok(_) => Ok(true),
            Err(IntersticeError::MigrationRequired(plan)) => {
                let Some(decide) = decide else {
                    return Err(IntersticeError::MigrationRequired(plan));
                };
                let policy = decide(name, &plan);
                if policy == MigrationPolicy::Safe {
                    // Abort: keep the old version.
                    let old = runtime.modules_path.as_ref().map(|p| p.join(name).join("module.wasm"));
                    let Some(old) = old.filter(|p| p.exists()) else {
                        return Err(IntersticeError::MigrationRequired(plan));
                    };
                    runtime.logger.log(
                        &format!("Module '{name}': update aborted, the old version is kept"),
                        LogSource::Node,
                        LogLevel::Warning,
                    );
                    let module = Module::from_file(runtime.clone(), &old).await?;
                    Runtime::load_module(runtime.clone(), module).await?;
                    return Ok(false);
                }
                let module = Module::from_bytes(runtime.clone(), &bytes).await?;
                Runtime::load_module_with(runtime.clone(), module, policy).await?;
                Ok(true)
            }
            Err(err) => Err(err),
        }
    }
}

use crate::runtime::reducer::ReducerJob;
use interstice_abi::IntersticeValue;

/// A running node, driven in process (DESIGN §10, Embedding). Calls go
/// through the scheduler like any other job, as the kernel: visibility
/// does not apply.
#[derive(Clone)]
pub struct NodeHandle {
    pub id: NodeId,
    runtime: Arc<Runtime>,
}

impl NodeHandle {
    /// Call a reducer; resolves once it has committed, or with its error.
    pub async fn call_reducer(
        &self,
        module: &str,
        reducer: &str,
        input: IntersticeValue,
    ) -> Result<(), IntersticeError> {
        let rx = self.submit_reducer(module, reducer, input)?;
        rx.await
            .map_err(|_| IntersticeError::Internal("reducer job dropped".into()))?
            .map_err(IntersticeError::Internal)
    }

    /// Queue a reducer call without waiting; the receiver yields its outcome.
    /// Blocks while the reducer queue is full.
    pub fn submit_reducer(
        &self,
        module: &str,
        reducer: &str,
        input: IntersticeValue,
    ) -> Result<tokio::sync::oneshot::Receiver<Result<(), String>>, IntersticeError> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.runtime
            .reducer_sender
            .send(
                ReducerJob {
                    module_name: module.to_string(),
                    reducer_name: reducer.to_string(),
                    input,
                    caller_node_id: self.id,
                    caller_module_name: String::new(),
                    admission: None,
                    completion: None,
                    reply: Some(crate::runtime::reducer::Reply::Local(tx)),
                }
                .into(),
            )
            .map_err(|_| IntersticeError::Internal("node stopped".into()))?;
        Ok(rx)
    }

    /// Call a query on a blocking thread.
    pub async fn call_query(
        &self,
        module: &str,
        query: &str,
        input: IntersticeValue,
    ) -> Result<IntersticeValue, IntersticeError> {
        let runtime = self.runtime.clone();
        let (module, query, id) = (module.to_string(), query.to_string(), self.id);
        tokio::task::spawn_blocking(move || runtime.call_query(&module, &query, input, id, ""))
            .await
            .map_err(|e| IntersticeError::Internal(e.to_string()))?
    }

    /// The names of the loaded modules.
    pub fn modules(&self) -> Vec<String> {
        self.runtime.modules.lock().keys().cloned().collect()
    }
}
