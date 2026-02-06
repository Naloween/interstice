use crate::{
    node::NodeId,
    runtime::{
        event::EventInstance,
        host_calls::{gpu::GpuState, input::from_winit::get_input_event_from_device_event},
    },
};
use pollster::FutureExt;
use std::{
    hash::{Hash, Hasher},
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc::UnboundedSender;
use winit::{
    application::ApplicationHandler,
    event::{DeviceEvent, DeviceId, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

pub struct App {
    node_id: NodeId,
    event_sender: UnboundedSender<EventInstance>,
    gpu: Arc<Mutex<Option<GpuState>>>,
}

impl App {
    pub fn new(
        node_id: NodeId,
        event_sender: UnboundedSender<EventInstance>,
        gpu: Arc<Mutex<Option<GpuState>>>,
    ) -> Self {
        Self {
            gpu,
            node_id,
            event_sender,
        }
    }

    pub fn run(mut self) {
        // Create local window and event loop
        let event_loop = EventLoop::new().unwrap();
        event_loop.set_control_flow(ControlFlow::Wait);

        event_loop.run_app(&mut self).expect("Event loop error");
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = event_loop
            .create_window(
                Window::default_attributes()
                    .with_title(format!("interstice - node({})", self.node_id)),
            )
            .expect("Failed to create window");
        window.request_redraw();
        let window = Arc::new(window);
        let gpu = GpuState::new(window.clone()).block_on();
        *self.gpu.lock().unwrap() = Some(gpu);
        self.event_sender
            .send(EventInstance::AppInitialized)
            .expect("Failed to send AppInitialized event");
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                // self.gpu.as_ref().unwrap().window.request_redraw();
                self.event_sender.send(EventInstance::Render).unwrap();
            }
            WindowEvent::Resized(size) => {
                let mut gpu = self.gpu.lock().unwrap();
                let gpu = gpu.as_mut().unwrap();
                gpu.graphics_end_frame();
                gpu.configure_surface(size.width, size.height);
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
        let mut hasher = std::hash::DefaultHasher::new();
        device_id.hash(&mut hasher);
        let device_id = hasher.finish() as u32;
        let input_event = get_input_event_from_device_event(device_id, event);
        self.event_sender
            .send(EventInstance::Input(input_event))
            .unwrap();
    }
}
