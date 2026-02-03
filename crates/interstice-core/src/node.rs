use crate::{
    authority::AuthorityEntry,
    error::IntersticeError,
    host_calls::gpu::GpuState,
    module::Module,
    network::{Network, NetworkHandle},
    persistence::TransactionLog,
    reducer::ReducerFrame,
    subscription::SubscriptionEventInstance,
    wasm::{StoreState, linker::define_host_calls},
};
use interstice_abi::{Authority, IntersticeValue, NodeSchema};
use std::{collections::HashMap, sync::Arc};
use std::{collections::VecDeque, path::Path};
use uuid::Uuid;
use wasmtime::{Engine, Linker};
use winit::event_loop::{ControlFlow, EventLoop};

pub type NodeId = Uuid;

pub struct Node {
    pub id: NodeId,
    pub adress: String,
    pub(crate) modules: HashMap<String, Module>,
    pub(crate) authority_modules: HashMap<Authority, AuthorityEntry>,
    pub(crate) call_stack: Vec<ReducerFrame>,
    pub(crate) transaction_logs: TransactionLog,
    pub(crate) engine: Arc<Engine>,
    pub(crate) linker: Linker<StoreState>,
    pub(crate) event_queue: VecDeque<SubscriptionEventInstance>,
    pub(crate) gpu: Option<GpuState>,
    pub(crate) network: Option<NetworkHandle>,
}

impl Node {
    pub fn new(transaction_log_path: &Path) -> Result<Self, IntersticeError> {
        let engine = Arc::new(Engine::default());
        let mut linker = Linker::new(&engine);
        define_host_calls(&mut linker).map_err(|err| {
            IntersticeError::Internal(format!("Couldn't add host calls to the linker: {}", err))
        })?;
        Ok(Self {
            id: Uuid::new_v4(),
            adress: "".into(),
            modules: HashMap::new(),
            authority_modules: HashMap::new(),
            call_stack: Vec::new(),
            engine,
            linker,
            transaction_logs: TransactionLog::new(transaction_log_path)?,
            event_queue: VecDeque::<SubscriptionEventInstance>::new(),
            gpu: None,
            network: None,
        })
    }

    pub fn schema(&self, name: String) -> NodeSchema {
        NodeSchema {
            name,
            adress: self.adress.clone(),
            modules: self.modules.values().map(|m| m.schema.clone()).collect(),
        }
    }

    pub fn clear_logs(&mut self) -> Result<(), IntersticeError> {
        self.transaction_logs.delete_all_logs()?;
        Ok(())
    }

    pub async fn start(&mut self, port: u32) -> Result<(), IntersticeError> {
        // Retreive the current state from logs
        self.replay()?;

        // create network and listen to events
        let mut network = Network::new();
        self.network = Some(network.get_handle());
        network
            .listen(&format!("0.0.0.0:{}", port), self.id)
            .await?;
        if port != 8080 {
            network
                .connect_to_peer("127.0.0.1:8080".into(), self.id)
                .await?;
        }
        network
            .run(|node_id, packet| println!("Received packet !"))
            .await;

        // Create local window and event loop
        let event_loop = EventLoop::new().unwrap();
        event_loop.set_control_flow(ControlFlow::Wait);

        event_loop.run_app(self).expect("Event loop error");

        Ok(())
    }

    pub fn run(
        &mut self,
        module: &str,
        reducer: &str,
        args: IntersticeValue,
    ) -> Result<IntersticeValue, IntersticeError> {
        let (result, events) = self.invoke_reducer(module, reducer, args)?;
        self.event_queue.extend(events);

        self.process_event_queue()?;

        Ok(result)
    }

    pub fn replay(&mut self) -> Result<(), IntersticeError> {
        let transactions = self.transaction_logs.read_all()?;
        println!("Replaying transactions: {:?}", transactions);

        for transaction in transactions {
            let _events = self.apply_transaction(transaction)?;
        }

        Ok(())
    }
}
