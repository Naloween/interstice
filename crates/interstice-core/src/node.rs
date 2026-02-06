use crate::{
    app::App,
    error::IntersticeError,
    network::{Network, NetworkHandle},
    runtime::{Runtime, event::EventInstance},
};
use interstice_abi::{ModuleSchema, NodeSchema};
use std::sync::Arc;
use std::{path::Path, sync::Mutex};
use tokio::sync::mpsc::{self, UnboundedReceiver};
use uuid::Uuid;

pub type NodeId = Uuid;

pub struct Node {
    pub id: NodeId,
    pub(crate) network_handle: NetworkHandle,
    pub(crate) event_sender: mpsc::UnboundedSender<EventInstance>,
    event_receiver: UnboundedReceiver<EventInstance>,
    network: Network,
    app: App,
    runtime: Arc<Runtime>,
}

impl Node {
    pub fn new(transaction_log_path: &Path, port: u32) -> Result<Self, IntersticeError> {
        let id = Uuid::new_v4();
        let address = "127.0.0.1:8080".to_string();
        // create network and listen to events
        let network = Network::new(id, address.clone());
        let network_handle = network.get_handle();

        let (event_sender, event_receiver) = mpsc::unbounded_channel();

        let gpu = Arc::new(Mutex::new(None));

        let app = App::new(id, event_sender.clone(), gpu.clone());

        let runtime = Arc::new(Runtime::new(
            transaction_log_path,
            event_sender.clone(),
            network_handle.clone(),
            gpu,
        )?);

        let node = Self {
            id,
            runtime,
            app,
            network,
            network_handle,
            event_sender,
            event_receiver,
        };

        Ok(node)
    }

    pub async fn start(self) -> Result<(), IntersticeError> {
        // Retreive the current state from logs
        self.runtime.replay()?;

        // // Run network events
        // self.network.listen()?;
        // let net_handle = self
        //     .network
        //     .run(|node_id, packet| println!("Received packet !"));
        let runtime_handle = Runtime::run(self.runtime, self.event_receiver);
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

    pub fn load_module(&self, path: &str) -> Result<ModuleSchema, IntersticeError> {
        Runtime::load_module(self.runtime.clone(), path)
    }
}
