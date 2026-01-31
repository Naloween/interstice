use interstice_abi::{
    BeginRenderPass, BufferUsage, CopyBufferToBuffer, CopyBufferToTexture, CopyTextureToBuffer,
    Draw, DrawIndexed, GpuId, SetIndexBuffer, SetVertexBuffer,
};
use std::{collections::HashMap, sync::Arc};
use wgpu::{SurfaceTexture, TextureView};
use winit::window::Window;

mod compute;
pub mod conversions;
pub mod dispatch;
mod general;
mod render;
mod ressource;

pub struct GpuState {
    pub window: Arc<Window>,
    next_id: GpuId,

    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,

    current_frame: Option<(SurfaceTexture, TextureView)>,
    surface_texture_id: Option<GpuId>,

    buffers: HashMap<GpuId, wgpu::Buffer>,
    textures: HashMap<GpuId, wgpu::Texture>,
    texture_views: HashMap<GpuId, wgpu::TextureView>,
    shaders: HashMap<GpuId, wgpu::ShaderModule>,
    bind_group_layouts: HashMap<GpuId, wgpu::BindGroupLayout>,
    bind_groups: HashMap<GpuId, wgpu::BindGroup>,
    pipeline_layouts: HashMap<GpuId, wgpu::PipelineLayout>,
    render_pipelines: HashMap<GpuId, wgpu::RenderPipeline>,
    compute_pipelines: HashMap<GpuId, wgpu::ComputePipeline>,

    encoders: HashMap<GpuId, EncoderState>,
}

struct EncoderState {
    encoder: wgpu::CommandEncoder,
    commands: Vec<EncoderCommand>,
    active_pass: Option<ActivePass>,
}

enum EncoderCommand {
    RenderPass(RenderPassState),
    ComputePass(ComputePassState),
    CopyBufferToBuffer(CopyBufferToBuffer),
    CopyBufferToTexture(CopyBufferToTexture),
    CopyTextureToBuffer(CopyTextureToBuffer),
}

enum ActivePass {
    Render(RenderPassState),
    Compute(ComputePassState),
}

pub struct RenderPassState {
    desc: BeginRenderPass,        // Your ABI struct
    commands: Vec<RenderCommand>, // Recorded draw commands
}

enum RenderCommand {
    SetPipeline(GpuId),
    SetBindGroup { index: u32, bind_group: GpuId },
    SetVertexBuffer(SetVertexBuffer),
    SetIndexBuffer(SetIndexBuffer),
    Draw(Draw),
    DrawIndexed(DrawIndexed),
}

pub struct ComputePassState {
    desc_encoder: GpuId,
    pipeline: Option<GpuId>,
    bind_groups: Vec<(u32, GpuId)>,
    dispatches: Vec<[u32; 3]>,
}

impl GpuState {
    pub async fn new(window: Arc<Window>) -> Self {
        let size = window.inner_size();

        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window.clone()).unwrap();

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
            })
            .await
            .unwrap();

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .unwrap();

        let surface_caps = surface.get_capabilities(&adapter);
        let format = surface_caps.formats[0];

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&device, &config);

        Self {
            instance,
            surface,
            device,
            queue,
            config,
            current_frame: None,
            window,
            next_id: 0,
            buffers: HashMap::new(),
            textures: HashMap::new(),
            texture_views: HashMap::new(),
            shaders: HashMap::new(),
            bind_group_layouts: HashMap::new(),
            bind_groups: HashMap::new(),
            pipeline_layouts: HashMap::new(),
            render_pipelines: HashMap::new(),
            compute_pipelines: HashMap::new(),
            encoders: HashMap::new(),
            surface_texture_id: None,
        }
    }

    fn alloc_id(&mut self) -> GpuId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn graphics_begin_frame(&mut self) {
        let frame = self
            .surface
            .get_current_texture()
            .expect("Failed to acquire frame");

        let view = frame.texture.create_view(&Default::default());
        self.current_frame = Some((frame, view));
    }

    pub fn graphics_end_frame(&mut self) {
        if let Some((frame, _)) = self.current_frame.take() {
            frame.present();
        }
        self.surface_texture_id = None;
    }

    fn get_current_surface_texture(&mut self) -> GpuId {
        let id = self.alloc_id();
        let (_, view) = self.current_frame.as_ref().expect("Frame not begun");

        self.texture_views.insert(id, view.clone());
        self.surface_texture_id = Some(id);
        id
    }

    pub fn submit(&mut self, encoder_id: GpuId) {
        let EncoderState {
            mut encoder,
            commands,
            ..
        } = self.encoders.remove(&encoder_id).unwrap();

        for cmd in commands {
            match cmd {
                EncoderCommand::RenderPass(rp) => {
                    self.execute_render_pass(&mut encoder, rp);
                }
                EncoderCommand::ComputePass(cp) => {
                    self.execute_compute_pass(&mut encoder, cp);
                }
                EncoderCommand::CopyBufferToBuffer(c) => {
                    let src = self.buffers.get(&c.src).unwrap();
                    let dst = self.buffers.get(&c.dst).unwrap();
                    encoder.copy_buffer_to_buffer(src, c.src_offset, dst, c.dst_offset, c.size);
                }
                EncoderCommand::CopyBufferToTexture(c) => {
                    self.execute_copy_buffer_to_texture(&mut encoder, c);
                }
                EncoderCommand::CopyTextureToBuffer(c) => {
                    self.execute_copy_texture_to_buffer(&mut encoder, c);
                }
            }
        }

        self.queue.submit(Some(encoder.finish()));
    }
}

fn map_buffer_usage(u: BufferUsage) -> wgpu::BufferUsages {
    let mut out = wgpu::BufferUsages::empty();

    if u.contains(BufferUsage::MAP_READ) {
        out |= wgpu::BufferUsages::MAP_READ;
    }
    if u.contains(BufferUsage::MAP_WRITE) {
        out |= wgpu::BufferUsages::MAP_WRITE;
    }
    if u.contains(BufferUsage::COPY_SRC) {
        out |= wgpu::BufferUsages::COPY_SRC;
    }
    if u.contains(BufferUsage::COPY_DST) {
        out |= wgpu::BufferUsages::COPY_DST;
    }
    if u.contains(BufferUsage::INDEX) {
        out |= wgpu::BufferUsages::INDEX;
    }
    if u.contains(BufferUsage::VERTEX) {
        out |= wgpu::BufferUsages::VERTEX;
    }
    if u.contains(BufferUsage::UNIFORM) {
        out |= wgpu::BufferUsages::UNIFORM;
    }
    if u.contains(BufferUsage::STORAGE) {
        out |= wgpu::BufferUsages::STORAGE;
    }
    if u.contains(BufferUsage::INDIRECT) {
        out |= wgpu::BufferUsages::INDIRECT;
    }

    out
}
