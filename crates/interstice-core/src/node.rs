use crate::{
    authority::AuthorityEntry,
    error::IntersticeError,
    host_calls::{gpu::GpuState, input::from_winit::get_input_event_from_device_event},
    module::Module,
    network::Network,
    persistence::TransactionLog,
    reducer::ReducerFrame,
    subscription::SubscriptionEventInstance,
    wasm::{StoreState, linker::define_host_calls},
};
use interstice_abi::{Authority, IntersticeValue};
use pollster::FutureExt;
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::Arc,
};
use std::{collections::VecDeque, path::Path};
use uuid::Uuid;
use wasmtime::{Engine, Linker};
use winit::{
    application::ApplicationHandler,
    event::{DeviceEvent, DeviceId, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

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
        })
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
        network
            .listen(&format!("0.0.0.0:{}", port), self.id)
            .await?;
        if port != 8080 {
            network
                .connect_to_peer("127.0.0.1:8080".into(), self.id)
                .await?;
        }
        network.run(|_, _| println!("Received packet !")).await;

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

impl ApplicationHandler for Node {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = event_loop
            .create_window(
                Window::default_attributes().with_title(format!("interstice - node({})", self.id)),
            )
            .expect("Failed to create window");
        window.request_redraw();
        let window = Arc::new(window);
        let gpu = GpuState::new(window.clone()).block_on();
        self.gpu = Some(gpu);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                if let Some(AuthorityEntry {
                    module_name: gpu_module_name,
                    on_event_reducer_name: Some(render_reducer_name),
                }) = self.authority_modules.get(&Authority::Gpu).cloned()
                {
                    self.run(
                        &gpu_module_name,
                        &render_reducer_name,
                        IntersticeValue::Vec(vec![]),
                    )
                    .unwrap();
                }
                self.gpu.as_ref().unwrap().window.request_redraw();

                // Temporary process event here at each frame
                match self.process_event_queue() {
                    Ok(_) => (),
                    Err(err) => println!("Error when processing events: {}", err),
                };
            }
            WindowEvent::Resized(size) => {
                self.gpu
                    .as_mut()
                    .unwrap()
                    .configure_surface(size.width, size.height);
            }
            _ => (),
        };
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        device_id: DeviceId,
        event: DeviceEvent,
    ) {
        if let Some(AuthorityEntry {
            module_name,
            on_event_reducer_name: Some(on_input_reducer_name),
        }) = self.authority_modules.get(&Authority::Input).cloned()
        {
            let module_name = module_name.clone();
            let mut hasher = std::hash::DefaultHasher::new();
            device_id.hash(&mut hasher);
            let device_id = hasher.finish() as u32;
            let input_event = get_input_event_from_device_event(device_id, event);
            match self.run(
                &module_name,
                &on_input_reducer_name,
                IntersticeValue::Vec(vec![input_event.into()]),
            ) {
                Ok(_) => (),
                Err(err) => println!("Error when running reducer: {}", err),
            }
        }
    }
}
