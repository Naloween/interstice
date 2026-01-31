use std::num::NonZero;

use interstice_abi::{CreateBuffer, GpuId};

use crate::host_calls::gpu::conversions::ToWgpu;

use super::{EncoderState, GpuState, map_buffer_usage};

impl GpuState {
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
}
