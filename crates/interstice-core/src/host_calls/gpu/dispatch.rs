use interstice_abi::GpuCall;

use crate::{Node, error::IntersticeError};

impl Node {
    pub fn handle_gpu_call(
        &mut self,
        call: interstice_abi::GpuCall,
    ) -> Result<Option<i64>, IntersticeError> {
        let gpu = self
            .gpu
            .as_mut()
            .ok_or_else(|| IntersticeError::Internal("GPU not initialized".into()))?;

        let potential_id = match call {
            GpuCall::CreateBuffer(desc) => {
                let id = gpu.create_buffer(desc);
                Some(id as i64)
            }
            GpuCall::WriteBuffer(w) => {
                gpu.write_buffer(w);
                None
            }
            GpuCall::CreateTexture(desc) => {
                let id = gpu.create_texture(desc);
                Some(id as i64)
            }
            GpuCall::CreateTextureView(v) => {
                let id = gpu.create_texture_view(v);
                Some(id as i64)
            }
            GpuCall::CreateShaderModule(s) => {
                let id = gpu.create_shader_module(s);
                Some(id as i64)
            }
            GpuCall::CreateBindGroupLayout(bgl) => {
                let id = gpu.create_bind_group_layout(bgl);
                Some(id as i64)
            }
            GpuCall::CreateBindGroup(bg) => {
                let id = gpu.create_bind_group(bg);
                Some(id as i64)
            }
            GpuCall::CreatePipelineLayout(pl) => {
                let id = gpu.create_pipeline_layout(pl);
                Some(id as i64)
            }
            GpuCall::CreateRenderPipeline(rp) => {
                let id = gpu.create_render_pipeline(rp);
                Some(id as i64)
            }
            GpuCall::CreateComputePipeline(cp) => {
                let id = gpu.create_compute_pipeline(cp);
                Some(id as i64)
            }
            GpuCall::CreateCommandEncoder => {
                let id = gpu.create_command_encoder();
                Some(id as i64)
            }
            GpuCall::BeginRenderPass(rp) => {
                gpu.begin_render_pass(rp);
                None
            }
            GpuCall::EndRenderPass { pass } => {
                gpu.end_render_pass(pass);
                None
            }
            GpuCall::SetRenderPipeline { pass, pipeline } => {
                gpu.set_render_pipeline(pass, pipeline);
                None
            }
            GpuCall::SetBindGroup {
                pass,
                index,
                bind_group,
            } => {
                gpu.set_bind_group(pass, index, bind_group);
                None
            }
            GpuCall::SetVertexBuffer(vb) => {
                gpu.set_vertex_buffer(vb);
                None
            }
            GpuCall::SetIndexBuffer(ib) => {
                gpu.set_index_buffer(ib);
                None
            }
            GpuCall::Draw(d) => {
                gpu.draw(d);
                None
            }
            GpuCall::DrawIndexed(d) => {
                gpu.draw_indexed(d);
                None
            }
            GpuCall::BeginComputePass { encoder } => {
                gpu.begin_compute_pass(encoder);
                None
            }
            GpuCall::EndComputePass { pass } => {
                gpu.end_compute_pass(pass);
                None
            }
            GpuCall::SetComputePipeline { pass, pipeline } => {
                gpu.set_compute_pipeline(pass, pipeline);
                None
            }

            GpuCall::Dispatch { pass, x, y, z } => {
                gpu.dispatch(pass, x, y, z);
                None
            }
            GpuCall::CopyBufferToBuffer(c) => {
                gpu.copy_buffer_to_buffer(c);
                None
            }
            GpuCall::CopyBufferToTexture(c) => {
                gpu.copy_buffer_to_texture(c);
                None
            }
            GpuCall::CopyTextureToBuffer(c) => {
                gpu.copy_texture_to_buffer(c);
                None
            }
            GpuCall::Submit { encoder } => {
                gpu.submit(encoder);
                None
            }
            GpuCall::Present => {
                gpu.graphics_end_frame();
                None
            }
            GpuCall::BeginFrame => {
                gpu.graphics_begin_frame();
                None
            }
            GpuCall::GetSurfaceFormat => {
                todo!();
                None
            }
            GpuCall::GetLimits => None,
            GpuCall::DestroyBuffer { id } => {
                todo!();
                None
            }
            GpuCall::DestroyTexture { id } => {
                todo!();
                None
            }
            GpuCall::WriteTexture(write_texture) => {
                todo!();
                None
            }
            GpuCall::GetCurrentSurfaceTexture => {
                todo!();
                None
            }
        };

        Ok(potential_id)
    }
}
