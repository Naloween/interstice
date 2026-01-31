use crate::{
    error::IntersticeError,
    host_calls::{gpu::GraphicsState, input::from_winit::get_input_event_from_device_event},
    module::Module,
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
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};

pub struct Node {
    pub id: Uuid,
    pub adress: String,
    pub(crate) modules: HashMap<String, Module>,
    pub(crate) authority_modules: HashMap<Authority, String>,
    pub(crate) call_stack: Vec<ReducerFrame>,
    pub(crate) transaction_logs: TransactionLog,
    pub(crate) engine: Arc<Engine>,
    pub(crate) linker: Linker<StoreState>,
    pub(crate) event_queue: VecDeque<SubscriptionEventInstance>,
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
        })
    }

    pub fn clear_logs(&mut self) -> Result<(), IntersticeError> {
        self.transaction_logs.delete_all_logs()?;
        Ok(())
    }

    pub fn start(&mut self) -> Result<(), IntersticeError> {
        self.replay()?;

        let event_loop = EventLoop::new().unwrap();
        let window = WindowBuilder::new()
            .with_title(format!("interstice - node({})", self.id))
            .build(&event_loop)
            .expect("Failed to create window");
        let mut gfx = GraphicsState::new(&window).block_on();
        gfx.window.request_redraw();

        event_loop.set_control_flow(ControlFlow::Wait);
        event_loop
            .run(|event, target| match event {
                Event::NewEvents(_) => {}
                Event::WindowEvent { event, .. } => match event {
                    WindowEvent::CloseRequested => {
                        target.exit();
                    }
                    WindowEvent::RedrawRequested => {
                        gfx.graphics_begin_frame();
                        gfx.graphics_end_frame();
                        gfx.window.request_redraw();

                        // Temporary process event here at each frame
                        match self.process_event_queue() {
                            Ok(_) => (),
                            Err(err) => println!("Error when processing events: {}", err),
                        };
                    }
                    _ => {}
                },
                Event::DeviceEvent { device_id, event } => {
                    if let Some(module_name) = self.authority_modules.get(&Authority::Input) {
                        let module_name = module_name.clone();
                        let mut hasher = std::hash::DefaultHasher::new();
                        device_id.hash(&mut hasher);
                        let device_id = hasher.finish() as u32;
                        let input_event = get_input_event_from_device_event(device_id, event);
                        match self.run(
                            &module_name,
                            "on_input",
                            IntersticeValue::Vec(vec![input_event.into()]),
                        ) {
                            Ok(_) => (),
                            Err(err) => println!("Error when running reducer: {}", err),
                        }
                    }
                }
                Event::UserEvent(_) => {}
                Event::Suspended => {}
                Event::Resumed => {}
                Event::AboutToWait => {}
                Event::LoopExiting => {}
                Event::MemoryWarning => {}
            })
            .expect("Couldn't start event loop");
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
