use interstice_abi::{
    CopyBufferToBuffer, CopyBufferToTexture, CopyTextureToBuffer, GpuId, SetIndexBuffer,
    SetVertexBuffer, WriteTexture,
};

use super::{ActivePass, EncoderCommand, GpuState, RenderCommand};

impl GpuState {
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

    pub fn execute_copy_buffer_to_texture(
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

    pub fn execute_copy_texture_to_buffer(
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

    pub fn write_texture(&mut self, desc: WriteTexture) {
        let texture = self.textures.get(&desc.texture).unwrap();

        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 0, y: 0, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            &desc.data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(desc.bytes_per_row),
                rows_per_image: Some(desc.rows_per_image),
            },
            wgpu::Extent3d {
                width: desc.width,
                height: desc.height,
                depth_or_array_layers: desc.depth,
            },
        );
    }

    pub fn set_index_buffer(&mut self, cmd: SetIndexBuffer) {
        let enc = self.encoders.get_mut(&cmd.pass).unwrap();

        match enc.active_pass.as_mut().unwrap() {
            ActivePass::Render(rp) => rp.commands.push(RenderCommand::SetIndexBuffer(cmd)),
            _ => panic!("Not in render pass"),
        }
    }
}
