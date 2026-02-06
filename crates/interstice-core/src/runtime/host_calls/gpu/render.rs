use interstice_abi::{BeginRenderPass, Draw, DrawIndexed, GpuId, IndexFormat, LoadOp, StoreOp};

use super::{
    ActivePass, EncoderCommand, GpuState, RenderCommand, RenderPassState, conversions::ToWgpu,
};

impl GpuState {
    pub fn create_render_pipeline(&mut self, desc: interstice_abi::CreateRenderPipeline) -> GpuId {
        let layout = self.pipeline_layouts.get(&desc.layout).unwrap();
        let vertex_shader = self.shaders.get(&desc.vertex.module).unwrap();
        let fragment_shader = self
            .shaders
            .get(&desc.fragment.as_ref().unwrap().module)
            .unwrap();

        let vertex_buffers: Vec<wgpu::VertexBufferLayout> =
            desc.vertex.buffers.iter().map(|v| v.to_wgpu()).collect();
        let vertex_buffers: &'static [wgpu::VertexBufferLayout] =
            Box::leak(vertex_buffers.into_boxed_slice());

        let frag_targets_vec: Vec<Option<wgpu::ColorTargetState>> = desc
            .fragment
            .as_ref()
            .unwrap()
            .targets
            .iter()
            .map(|c| Some(c.to_wgpu()))
            .collect();

        let frag_entry_point = desc
            .fragment
            .as_ref()
            .map(|f| Box::leak(f.entry_point.clone().into_boxed_str()) as &str);
        let label = desc.label.map(|s| Box::leak(s.into_boxed_str()) as &str);

        let pipeline = self
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label,
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: vertex_shader,
                    entry_point: Some(&desc.vertex.entry_point),
                    buffers: &vertex_buffers,
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: fragment_shader,
                    entry_point: frag_entry_point,
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

    pub fn begin_render_pass(&mut self, desc: BeginRenderPass) -> GpuId {
        let pass_id = desc.encoder;
        let enc = self.encoders.get_mut(&desc.encoder).unwrap();

        assert!(enc.active_pass.is_none(), "Pass already active");

        enc.active_pass = Some(ActivePass::Render(RenderPassState {
            desc,
            commands: Vec::new(),
        }));

        pass_id as GpuId
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

    pub fn draw_indexed(&mut self, cmd: DrawIndexed) {
        let enc = self.encoders.get_mut(&cmd.pass).unwrap();

        match enc.active_pass.as_mut().unwrap() {
            ActivePass::Render(rp) => rp.commands.push(RenderCommand::DrawIndexed(cmd)),
            _ => panic!("Not in render pass"),
        }
    }

    pub fn execute_render_pass(&mut self, encoder: &mut wgpu::CommandEncoder, rp: RenderPassState) {
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

    pub fn end_render_pass(&mut self, encoder_id: GpuId) {
        let enc = self.encoders.get_mut(&encoder_id).unwrap();

        match enc.active_pass.take() {
            Some(ActivePass::Render(rp)) => {
                enc.commands.push(EncoderCommand::RenderPass(rp));
            }
            _ => panic!("No render pass active"),
        }
    }
}
