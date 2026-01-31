use interstice_abi::{
    BeginRenderPass, BufferUsage, CopyBufferToBuffer, CopyBufferToTexture, CopyTextureToBuffer,
    CreateBuffer, Draw, DrawIndexed, GpuId, IndexFormat, LoadOp, SetIndexBuffer, SetVertexBuffer,
    StoreOp,
};
use std::{collections::HashMap, num::NonZero, sync::Arc};
use wgpu::{SurfaceTexture, TextureView};
use winit::window::Window;

use crate::host_calls::gpu::conversions::ToWgpu;

pub mod conversions;
pub mod dispatch;

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

struct RenderPassState {
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

struct ComputePassState {
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

    pub fn create_buffer(&mut self, desc: CreateBuffer) -> GpuId {
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: desc.size,
            usage: map_buffer_usage(desc.usage),
            mapped_at_creation: desc.mapped_at_creation,
        });

        let id = self.alloc_id();
        self.buffers.insert(id, buffer);
        id
    }

    pub fn create_command_encoder(&mut self) -> GpuId {
        let encoder = self.device.create_command_encoder(&Default::default());
        let id = self.alloc_id();

        self.encoders.insert(
            id,
            EncoderState {
                encoder,
                commands: Vec::new(),
                active_pass: None,
            },
        );

        id
    }

    pub fn begin_render_pass(&mut self, desc: BeginRenderPass) {
        let enc = self.encoders.get_mut(&desc.encoder).unwrap();

        assert!(enc.active_pass.is_none(), "Pass already active");

        enc.active_pass = Some(ActivePass::Render(RenderPassState {
            desc,
            commands: Vec::new(),
        }));
    }

    pub fn set_render_pipeline(&mut self, pass_id: GpuId, pipeline: GpuId) {
        let enc = self.encoders.get_mut(&pass_id).unwrap();

        match enc.active_pass.as_mut().unwrap() {
            ActivePass::Render(rp) => rp.commands.push(RenderCommand::SetPipeline(pipeline)),
            _ => panic!("Not in render pass"),
        }
    }

    pub fn draw(&mut self, draw: Draw) {
        let enc = self.encoders.get_mut(&draw.pass).unwrap();

        match enc.active_pass.as_mut().unwrap() {
            ActivePass::Render(rp) => rp.commands.push(RenderCommand::Draw(draw)),
            _ => panic!("Not in render pass"),
        }
    }

    pub fn end_render_pass(&mut self, encoder_id: GpuId) {
        let enc = self.encoders.get_mut(&encoder_id).unwrap();

        match enc.active_pass.take() {
            Some(ActivePass::Render(rp)) => {
                enc.commands.push(EncoderCommand::RenderPass(rp));
            }
            _ => panic!("No render pass active"),
        }
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

    fn execute_render_pass(&mut self, encoder: &mut wgpu::CommandEncoder, rp: RenderPassState) {
        let color_attachments: Vec<_> = rp
            .desc
            .color_attachments
            .iter()
            .map(|att| {
                if let Some(view) = self.texture_views.get(&att.view) {
                    Some(wgpu::RenderPassColorAttachment {
                        view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: match att.load {
                                LoadOp::Load => wgpu::LoadOp::Load,
                                LoadOp::Clear => wgpu::LoadOp::Clear(wgpu::Color {
                                    r: att.clear_color[0] as f64,
                                    g: att.clear_color[1] as f64,
                                    b: att.clear_color[2] as f64,
                                    a: att.clear_color[3] as f64,
                                }),
                            },
                            store: match att.store {
                                StoreOp::Store => wgpu::StoreOp::Store,
                                StoreOp::Discard => wgpu::StoreOp::Discard,
                            },
                        },
                        depth_slice: None,
                    })
                } else {
                    None
                }
            })
            .collect();

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &color_attachments,
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
            multiview_mask: None,
        });

        for cmd in rp.commands {
            match cmd {
                RenderCommand::SetPipeline(id) => {
                    let pipeline = self.render_pipelines.get(&id).unwrap();
                    pass.set_pipeline(pipeline);
                }
                RenderCommand::Draw(d) => {
                    pass.draw(
                        d.first_vertex..d.first_vertex + d.vertices,
                        d.first_instance..d.first_instance + d.instances,
                    );
                }
                RenderCommand::SetBindGroup { index, bind_group } => {
                    let bg = self.bind_groups.get(&bind_group).unwrap();
                    pass.set_bind_group(index, bg, &[]);
                }
                RenderCommand::SetVertexBuffer(vb) => {
                    let buf = self.buffers.get(&vb.buffer).unwrap();
                    pass.set_vertex_buffer(vb.slot, buf.slice(vb.offset..));
                }
                RenderCommand::SetIndexBuffer(ib) => {
                    let buf = self.buffers.get(&ib.buffer).unwrap();
                    pass.set_index_buffer(
                        buf.slice(ib.offset..),
                        match ib.index_format {
                            IndexFormat::Uint16 => wgpu::IndexFormat::Uint16,
                            IndexFormat::Uint32 => wgpu::IndexFormat::Uint32,
                        },
                    );
                }
                RenderCommand::DrawIndexed(d) => {
                    pass.draw_indexed(
                        d.first_index..d.first_index + d.indices,
                        d.base_vertex,
                        d.first_instance..d.first_instance + d.instances,
                    );
                }
            }
        }
    }

    pub fn set_bind_group(&mut self, pass_id: GpuId, index: u32, bind_group: GpuId) {
        let enc = self.encoders.get_mut(&pass_id).unwrap();

        match enc.active_pass.as_mut().unwrap() {
            ActivePass::Render(rp) => {
                rp.commands
                    .push(RenderCommand::SetBindGroup { index, bind_group });
            }
            _ => panic!("Not in render pass"),
        }
    }

    pub fn set_vertex_buffer(&mut self, cmd: SetVertexBuffer) {
        let enc = self.encoders.get_mut(&cmd.pass).unwrap();

        match enc.active_pass.as_mut().unwrap() {
            ActivePass::Render(rp) => rp.commands.push(RenderCommand::SetVertexBuffer(cmd)),
            _ => panic!("Not in render pass"),
        }
    }

    pub fn draw_indexed(&mut self, cmd: DrawIndexed) {
        let enc = self.encoders.get_mut(&cmd.pass).unwrap();

        match enc.active_pass.as_mut().unwrap() {
            ActivePass::Render(rp) => rp.commands.push(RenderCommand::DrawIndexed(cmd)),
            _ => panic!("Not in render pass"),
        }
    }

    pub fn begin_compute_pass(&mut self, encoder: GpuId) {
        let enc = self.encoders.get_mut(&encoder).unwrap();
        assert!(enc.active_pass.is_none());

        enc.active_pass = Some(ActivePass::Compute(ComputePassState {
            desc_encoder: encoder,
            pipeline: None,
            bind_groups: Vec::new(),
            dispatches: Vec::new(),
        }));
    }

    pub fn set_compute_pipeline(&mut self, pass: GpuId, pipeline: GpuId) {
        let enc = self.encoders.get_mut(&pass).unwrap();

        match enc.active_pass.as_mut().unwrap() {
            ActivePass::Compute(cp) => cp.pipeline = Some(pipeline),
            _ => panic!("Not compute pass"),
        }
    }

    pub fn dispatch(&mut self, pass: GpuId, x: u32, y: u32, z: u32) {
        let enc = self.encoders.get_mut(&pass).unwrap();

        match enc.active_pass.as_mut().unwrap() {
            ActivePass::Compute(cp) => cp.dispatches.push([x, y, z]),
            _ => panic!("Not compute pass"),
        }
    }

    pub fn end_compute_pass(&mut self, encoder_id: GpuId) {
        let enc = self.encoders.get_mut(&encoder_id).unwrap();

        match enc.active_pass.take() {
            Some(ActivePass::Compute(cp)) => {
                enc.commands.push(EncoderCommand::ComputePass(cp));
            }
            _ => panic!("No compute pass active"),
        }
    }

    pub fn copy_buffer_to_buffer(&mut self, cmd: CopyBufferToBuffer) {
        let enc = self.encoders.get_mut(&cmd.encoder).unwrap();
        enc.commands.push(EncoderCommand::CopyBufferToBuffer(cmd));
    }

    pub fn copy_buffer_to_texture(&mut self, cmd: CopyBufferToTexture) {
        let enc = self.encoders.get_mut(&cmd.encoder).unwrap();
        enc.commands.push(EncoderCommand::CopyBufferToTexture(cmd));
    }

    pub fn copy_texture_to_buffer(&mut self, cmd: CopyTextureToBuffer) {
        let enc = self.encoders.get_mut(&cmd.encoder).unwrap();
        enc.commands.push(EncoderCommand::CopyTextureToBuffer(cmd));
    }

    fn execute_compute_pass(&mut self, encoder: &mut wgpu::CommandEncoder, cp: ComputePassState) {
        let mut pass = encoder.begin_compute_pass(&Default::default());

        if let Some(pipeline_id) = cp.pipeline {
            let pipeline = self.compute_pipelines.get(&pipeline_id).unwrap();
            pass.set_pipeline(pipeline);
        }

        for (index, bg_id) in cp.bind_groups {
            let bg = self.bind_groups.get(&bg_id).unwrap();
            pass.set_bind_group(index, bg, &[]);
        }

        for [x, y, z] in cp.dispatches {
            pass.dispatch_workgroups(x, y, z);
        }
    }
    fn execute_copy_buffer_to_texture(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        c: CopyBufferToTexture,
    ) {
        let buffer = self.buffers.get(&c.src_buffer).unwrap();
        let texture = self.textures.get(&c.dst_texture).unwrap();

        encoder.copy_buffer_to_texture(
            wgpu::TexelCopyBufferInfo {
                buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: c.src_offset,
                    bytes_per_row: Some(c.bytes_per_row),
                    rows_per_image: Some(c.rows_per_image),
                },
            },
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: c.mip_level,
                origin: wgpu::Origin3d {
                    x: c.origin[0],
                    y: c.origin[1],
                    z: c.origin[2],
                },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: c.extent[0],
                height: c.extent[1],
                depth_or_array_layers: c.extent[2],
            },
        );
    }

    fn execute_copy_texture_to_buffer(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        c: CopyTextureToBuffer,
    ) {
        let texture = self.textures.get(&c.src_texture).unwrap();
        let buffer = self.buffers.get(&c.dst_buffer).unwrap();

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: c.mip_level,
                origin: wgpu::Origin3d {
                    x: c.origin[0],
                    y: c.origin[1],
                    z: c.origin[2],
                },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: c.dst_offset,
                    bytes_per_row: Some(c.bytes_per_row),
                    rows_per_image: Some(c.rows_per_image),
                },
            },
            wgpu::Extent3d {
                width: c.extent[0],
                height: c.extent[1],
                depth_or_array_layers: c.extent[2],
            },
        );
    }

    pub fn write_buffer(&mut self, w: interstice_abi::WriteBuffer) {
        let buffer = self.buffers.get(&w.buffer).unwrap();
        self.queue.write_buffer(buffer, w.offset, &w.data);
    }

    pub fn create_texture(&mut self, desc: interstice_abi::CreateTexture) -> GpuId {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: desc.width,
                height: desc.height,
                depth_or_array_layers: desc.depth,
            },
            mip_level_count: desc.mip_levels,
            sample_count: desc.sample_count,
            dimension: desc.dimension.to_wgpu(),
            format: desc.format.to_wgpu(),
            usage: desc.usage.to_wgpu(),
            view_formats: &[],
        });

        let id = self.alloc_id();
        self.textures.insert(id, texture);
        return id;
    }

    pub fn create_texture_view(&mut self, desc: interstice_abi::CreateTextureView) -> GpuId {
        let texture = self.textures.get(&desc.texture).unwrap();

        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: None,
            format: None,
            dimension: desc.dimension.map(|v| v.to_wgpu()),
            aspect: wgpu::TextureAspect::All,
            base_mip_level: desc.base_mip_level,
            mip_level_count: desc.mip_level_count,
            base_array_layer: 0,
            array_layer_count: None,
            usage: None,
        });

        let id = self.alloc_id();

        self.texture_views.insert(id, view);

        return id;
    }

    pub fn create_shader_module(&mut self, desc: interstice_abi::CreateShaderModule) -> GpuId {
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Wgsl(desc.wgsl_source.into()),
            });

        let id = self.alloc_id();

        self.shaders.insert(id, module);

        return id;
    }

    pub fn create_bind_group_layout(
        &mut self,
        desc: interstice_abi::CreateBindGroupLayout,
    ) -> GpuId {
        let entries: Vec<_> = desc.entries.iter().map(|e| e.to_wgpu()).collect();

        let layout = self
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &entries,
            });

        let id = self.alloc_id();

        self.bind_group_layouts.insert(id, layout);
        return id;
    }

    pub fn create_bind_group(&mut self, desc: interstice_abi::CreateBindGroup) -> GpuId {
        let layout = self.bind_group_layouts.get(&desc.layout).unwrap();

        let entries: Vec<_> = desc
            .entries
            .iter()
            .map(|e| {
                let resource = match e.resource {
                    interstice_abi::BindingResource::Buffer {
                        buffer,
                        offset,
                        size,
                    } => {
                        let buf = self.buffers.get(&buffer).unwrap();
                        wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: buf,
                            offset,
                            size: size.map(|s| NonZero::new(s).unwrap()),
                        })
                    }
                    interstice_abi::BindingResource::TextureView(view) => {
                        wgpu::BindingResource::TextureView(self.texture_views.get(&view).unwrap())
                    }
                    interstice_abi::BindingResource::Sampler(_) => todo!(),
                };

                wgpu::BindGroupEntry {
                    binding: e.binding,
                    resource,
                }
            })
            .collect();

        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout,
            entries: &entries,
        });

        let id = self.alloc_id();
        self.bind_groups.insert(id, bg);
        return id;
    }

    pub fn create_pipeline_layout(&mut self, desc: interstice_abi::CreatePipelineLayout) -> GpuId {
        let layouts: Vec<_> = desc
            .bind_group_layouts
            .iter()
            .map(|id| self.bind_group_layouts.get(id).unwrap())
            .collect();

        let layout = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &layouts,
                immediate_size: 0,
            });

        let id = self.alloc_id();

        self.pipeline_layouts.insert(id, layout);
        return id;
    }

    pub fn create_render_pipeline(&mut self, desc: interstice_abi::CreateRenderPipeline) -> GpuId {
        let layout = self.pipeline_layouts.get(&desc.layout).unwrap();
        let vertex_shader = self.shaders.get(&desc.vertex.module).unwrap();
        let fragment_shader = self.shaders.get(&desc.fragment.module).unwrap();

        let vertex_buffers: Vec<wgpu::VertexBufferLayout> =
            desc.vertex.buffers.iter().map(|v| v.to_wgpu()).collect();
        let vertex_buffers: &'static [wgpu::VertexBufferLayout] =
            Box::leak(vertex_buffers.into_boxed_slice());

        let frag_targets_vec: Vec<Option<wgpu::ColorTargetState>> = desc
            .fragment
            .targets
            .iter()
            .map(|c| Some(c.to_wgpu()))
            .collect();

        let pipeline = self
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None,
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: vertex_shader,
                    entry_point: Some(&desc.vertex.entry_point),
                    buffers: &vertex_buffers,
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: fragment_shader,
                    entry_point: Some(&desc.fragment.entry_point),
                    targets: &frag_targets_vec,
                    compilation_options: Default::default(),
                }),
                primitive: desc.primitive.to_wgpu(),
                depth_stencil: desc.depth_stencil.map(|v| v.to_wgpu()),
                multisample: desc.multisample.to_wgpu(),
                multiview_mask: None,
                cache: None,
            });

        let id = self.alloc_id();
        self.render_pipelines.insert(id, pipeline);
        return id;
    }

    pub fn create_compute_pipeline(
        &mut self,
        desc: interstice_abi::CreateComputePipeline,
    ) -> GpuId {
        let layout = self.pipeline_layouts.get(&desc.layout).unwrap();
        let shader = self.shaders.get(&desc.module).unwrap();

        let pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: None,
                layout: Some(layout),
                module: shader,
                entry_point: Some(&desc.entry_point),
                compilation_options: Default::default(),
                cache: None,
            });

        let id = self.alloc_id();
        self.compute_pipelines.insert(id, pipeline);
        return id;
    }

    pub fn set_index_buffer(&mut self, cmd: SetIndexBuffer) {
        let enc = self.encoders.get_mut(&cmd.pass).unwrap();

        match enc.active_pass.as_mut().unwrap() {
            ActivePass::Render(rp) => rp.commands.push(RenderCommand::SetIndexBuffer(cmd)),
            _ => panic!("Not in render pass"),
        }
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
