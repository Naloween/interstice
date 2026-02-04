pub mod from_winit;

use crate::{
    Node,
    authority::AuthorityEntry,
    host_calls::{gpu::GpuState, input::from_winit::get_input_event_from_device_event},
};
use interstice_abi::{Authority, IntersticeValue};
use pollster::FutureExt;
use std::{
    hash::{Hash, Hasher},
    sync::Arc,
};
use winit::{
    application::ApplicationHandler,
    event::{DeviceEvent, DeviceId, WindowEvent},
    event_loop::ActiveEventLoop,
    window::{Window, WindowId},
};

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
                    .expect("Running the render reducer failed");
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
