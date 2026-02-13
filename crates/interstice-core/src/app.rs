use crate::{
    node::NodeId,
    runtime::{
        AuthorityEntry, Runtime,
        event::EventInstance,
        host_calls::{
            gpu::{GpuCallRequest, GpuCallResult, GpuState},
            input::from_winit::get_input_event_from_device_event,
        },
        reducer::ReducerJob,
    },
};
use interstice_abi::{Authority, IntersticeValue};
use pollster::FutureExt;
use std::{
    hash::{Hash, Hasher},
    sync::{Arc, Mutex, mpsc::Receiver},
    time::Duration,
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
    runtime: Arc<Runtime>,
    gpu_call_receiver: Receiver<GpuCallRequest>,
}

impl App {
    pub fn new(
        node_id: NodeId,
        event_sender: UnboundedSender<EventInstance>,
        gpu: Arc<Mutex<Option<GpuState>>>,
        runtime: Arc<Runtime>,
        gpu_call_receiver: Receiver<GpuCallRequest>,
    ) -> Self {
        Self {
            gpu,
            node_id,
            event_sender,
            runtime,
            gpu_call_receiver,
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
        window.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        self.drain_gpu_calls();
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                let render_target = {
                    self.runtime
                        .authority_modules
                        .lock()
                        .unwrap()
                        .get(&Authority::Gpu)
                        .cloned()
                        .and_then(|entry| match entry {
                            AuthorityEntry::Gpu {
                                module_name,
                                render_reducer: Some(reducer),
                            } => Some((module_name, reducer)),
                            _ => None,
                        })
                };

                if let Some((module_name, reducer_name)) = render_target {
                    let (done_tx, done_rx) = std::sync::mpsc::channel();
                    let send_result = self.runtime.reducer_sender.send(ReducerJob {
                        module_name,
                        reducer_name,
                        input: IntersticeValue::Vec(vec![]),
                        caller_node_id: self.node_id,
                        completion: Some(done_tx),
                    });
                    let _ = send_result;
                    self.wait_for_render_completion(done_rx);
                }
            }
            WindowEvent::Resized(size) => {
                let mut gpu = self.gpu.lock().unwrap();
                let gpu = gpu.as_mut().unwrap();
                gpu.graphics_end_frame();
                gpu.configure_surface(size.width.max(1), size.height.max(1));
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
        self.drain_gpu_calls();
        let mut hasher = std::hash::DefaultHasher::new();
        device_id.hash(&mut hasher);
        let device_id = hasher.finish() as u32;
        let input_event = get_input_event_from_device_event(device_id, event);
        self.event_sender
            .send(EventInstance::Input(input_event))
            .unwrap();
    }
}

impl App {
    fn wait_for_render_completion(&mut self, done_rx: std::sync::mpsc::Receiver<()>) {
        loop {
            self.drain_gpu_calls();
            match done_rx.recv_timeout(Duration::from_millis(1)) {
                Ok(()) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    }

    fn drain_gpu_calls(&mut self) {
        while let Ok(req) = self.gpu_call_receiver.try_recv() {
            let result = self.execute_gpu_call(req.call);
            let _ = req.respond_to.send(result);
        }
    }

    fn execute_gpu_call(
        &mut self,
        call: interstice_abi::GpuCall,
    ) -> Result<GpuCallResult, crate::IntersticeError> {
        let mut gpu = self.gpu.lock().unwrap();
        let gpu = gpu
            .as_mut()
            .ok_or_else(|| crate::IntersticeError::Internal("GPU not initialized".into()))?;

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match call {
            interstice_abi::GpuCall::CreateBuffer(desc) => {
                let id = gpu.create_buffer(desc);
                Ok(GpuCallResult::I64(id as i64))
            }
            interstice_abi::GpuCall::WriteBuffer(w) => {
                gpu.write_buffer(w);
                Ok(GpuCallResult::None)
            }
            interstice_abi::GpuCall::CreateTexture(desc) => {
                let id = gpu.create_texture(desc);
                Ok(GpuCallResult::I64(id as i64))
            }
            interstice_abi::GpuCall::CreateTextureView(v) => {
                let id = gpu.create_texture_view(v);
                Ok(GpuCallResult::I64(id as i64))
            }
            interstice_abi::GpuCall::CreateShaderModule(s) => {
                let id = gpu.create_shader_module(s);
                Ok(GpuCallResult::I64(id as i64))
            }
            interstice_abi::GpuCall::CreateBindGroupLayout(bgl) => {
                let id = gpu.create_bind_group_layout(bgl);
                Ok(GpuCallResult::I64(id as i64))
            }
            interstice_abi::GpuCall::CreateBindGroup(bg) => {
                let id = gpu.create_bind_group(bg);
                Ok(GpuCallResult::I64(id as i64))
            }
            interstice_abi::GpuCall::CreatePipelineLayout(pl) => {
                let id = gpu.create_pipeline_layout(pl);
                Ok(GpuCallResult::I64(id as i64))
            }
            interstice_abi::GpuCall::CreateRenderPipeline(rp) => {
                let id = gpu.create_render_pipeline(rp);
                Ok(GpuCallResult::I64(id as i64))
            }
            interstice_abi::GpuCall::CreateComputePipeline(cp) => {
                let id = gpu.create_compute_pipeline(cp);
                Ok(GpuCallResult::I64(id as i64))
            }
            interstice_abi::GpuCall::CreateCommandEncoder => {
                let id = gpu.create_command_encoder();
                Ok(GpuCallResult::I64(id as i64))
            }
            interstice_abi::GpuCall::BeginRenderPass(rp) => {
                let id = gpu.begin_render_pass(rp);
                Ok(GpuCallResult::I64(id as i64))
            }
            interstice_abi::GpuCall::EndRenderPass { pass } => {
                gpu.end_render_pass(pass);
                Ok(GpuCallResult::None)
            }
            interstice_abi::GpuCall::SetRenderPipeline { pass, pipeline } => {
                gpu.set_render_pipeline(pass, pipeline);
                Ok(GpuCallResult::None)
            }
            interstice_abi::GpuCall::SetBindGroup {
                pass,
                index,
                bind_group,
            } => {
                gpu.set_bind_group(pass, index, bind_group);
                Ok(GpuCallResult::None)
            }
            interstice_abi::GpuCall::SetVertexBuffer(vb) => {
                gpu.set_vertex_buffer(vb);
                Ok(GpuCallResult::None)
            }
            interstice_abi::GpuCall::SetIndexBuffer(ib) => {
                gpu.set_index_buffer(ib);
                Ok(GpuCallResult::None)
            }
            interstice_abi::GpuCall::Draw(d) => {
                gpu.draw(d);
                Ok(GpuCallResult::None)
            }
            interstice_abi::GpuCall::DrawIndexed(d) => {
                gpu.draw_indexed(d);
                Ok(GpuCallResult::None)
            }
            interstice_abi::GpuCall::BeginComputePass { encoder } => {
                gpu.begin_compute_pass(encoder);
                Ok(GpuCallResult::None)
            }
            interstice_abi::GpuCall::EndComputePass { pass } => {
                gpu.end_compute_pass(pass);
                Ok(GpuCallResult::None)
            }
            interstice_abi::GpuCall::SetComputePipeline { pass, pipeline } => {
                gpu.set_compute_pipeline(pass, pipeline);
                Ok(GpuCallResult::None)
            }
            interstice_abi::GpuCall::Dispatch { pass, x, y, z } => {
                gpu.dispatch(pass, x, y, z);
                Ok(GpuCallResult::None)
            }
            interstice_abi::GpuCall::CopyBufferToBuffer(c) => {
                gpu.copy_buffer_to_buffer(c);
                Ok(GpuCallResult::None)
            }
            interstice_abi::GpuCall::CopyBufferToTexture(c) => {
                gpu.copy_buffer_to_texture(c);
                Ok(GpuCallResult::None)
            }
            interstice_abi::GpuCall::CopyTextureToBuffer(c) => {
                gpu.copy_texture_to_buffer(c);
                Ok(GpuCallResult::None)
            }
            interstice_abi::GpuCall::Submit { encoder } => {
                gpu.submit(encoder);
                Ok(GpuCallResult::None)
            }
            interstice_abi::GpuCall::Present => {
                gpu.graphics_end_frame();
                Ok(GpuCallResult::None)
            }
            interstice_abi::GpuCall::BeginFrame => {
                gpu.graphics_begin_frame();
                Ok(GpuCallResult::None)
            }
            interstice_abi::GpuCall::GetSurfaceFormat => {
                let format = gpu.get_surface_format();
                Ok(GpuCallResult::TextureFormat(format))
            }
            interstice_abi::GpuCall::GetLimits => Ok(GpuCallResult::None),
            interstice_abi::GpuCall::DestroyBuffer { .. } => Err(crate::IntersticeError::Internal(
                "DestroyBuffer not implemented".into(),
            )),
            interstice_abi::GpuCall::DestroyTexture { .. } => Err(
                crate::IntersticeError::Internal("DestroyTexture not implemented".into()),
            ),
            interstice_abi::GpuCall::WriteTexture(_) => Err(crate::IntersticeError::Internal(
                "WriteTexture not implemented".into(),
            )),
            interstice_abi::GpuCall::GetCurrentSurfaceTexture => {
                let id = gpu.get_current_surface_texture();
                Ok(GpuCallResult::I64(id as i64))
            }
        }));

        match result {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(crate::IntersticeError::Internal("GPU call panicked".into())),
        }
    }
}
