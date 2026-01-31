use interstice_abi::GpuCall;

use crate::{Node, error::IntersticeError};

impl Node {
    pub fn handle_gpu_call(
        &mut self,
        call: interstice_abi::GpuCall,
    ) -> Result<(), IntersticeError> {
        let gpu = self.gpu.as_mut().unwrap();

        match call {
            GpuCall::CreateBuffer(desc) => {
                let a = gpu.create_buffer(desc);
            }
            GpuCall::WriteBuffer(w) => {
                let a = gpu.write_buffer(w);
            }
            GpuCall::CreateTexture(desc) => {
                let a = gpu.create_texture(desc);
            }
            GpuCall::CreateTextureView(v) => {
                let a = gpu.create_texture_view(v);
            }
            GpuCall::CreateShaderModule(s) => {
                let a = gpu.create_shader_module(s);
            }
            GpuCall::CreateBindGroupLayout(bgl) => {
                let a = gpu.create_bind_group_layout(bgl);
            }
            GpuCall::CreateBindGroup(bg) => {
                let a = gpu.create_bind_group(bg);
            }
            GpuCall::CreatePipelineLayout(pl) => {
                let a = gpu.create_pipeline_layout(pl);
            }
            GpuCall::CreateRenderPipeline(rp) => {
                let a = gpu.create_render_pipeline(rp);
            }
            GpuCall::CreateComputePipeline(cp) => {
                let a = gpu.create_compute_pipeline(cp);
            }
            GpuCall::CreateCommandEncoder => {
                let a = gpu.create_command_encoder();
            }
            GpuCall::BeginRenderPass(rp) => {
                let a = gpu.begin_render_pass(rp);
            }
            GpuCall::EndRenderPass { pass } => gpu.end_render_pass(pass),
            GpuCall::SetRenderPipeline { pass, pipeline } => {
                gpu.set_render_pipeline(pass, pipeline)
            }
            GpuCall::SetBindGroup {
                pass,
                index,
                bind_group,
            } => gpu.set_bind_group(pass, index, bind_group),
            GpuCall::SetVertexBuffer(vb) => gpu.set_vertex_buffer(vb),
            GpuCall::SetIndexBuffer(ib) => gpu.set_index_buffer(ib),
            GpuCall::Draw(d) => gpu.draw(d),
            GpuCall::DrawIndexed(d) => gpu.draw_indexed(d),
            GpuCall::BeginComputePass { encoder } => gpu.begin_compute_pass(encoder),
            GpuCall::EndComputePass { pass } => gpu.end_compute_pass(pass),
            GpuCall::SetComputePipeline { pass, pipeline } => {
                gpu.set_compute_pipeline(pass, pipeline)
            }
            GpuCall::Dispatch { pass, x, y, z } => gpu.dispatch(pass, x, y, z),
            GpuCall::CopyBufferToBuffer(c) => gpu.copy_buffer_to_buffer(c),
            GpuCall::CopyBufferToTexture(c) => gpu.copy_buffer_to_texture(c),
            GpuCall::CopyTextureToBuffer(c) => gpu.copy_texture_to_buffer(c),
            GpuCall::Submit { encoder } => gpu.submit(encoder),
            GpuCall::Present => gpu.graphics_end_frame(),
            GpuCall::BeginFrame => gpu.graphics_begin_frame(),
            GpuCall::GetSurfaceFormat => todo!(),
            GpuCall::GetLimits => todo!(),
            GpuCall::DestroyBuffer { id } => todo!(),
            GpuCall::DestroyTexture { id } => todo!(),
            GpuCall::WriteTexture(write_texture) => todo!(),
            GpuCall::GetCurrentSurfaceTexture => todo!(),
        }

        Ok(())
    }
}
