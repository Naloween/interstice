use crate::host_calls::host_call;
use interstice_abi::*;

pub fn begin_frame() {
    host_call(HostCall::Gpu(GpuCall::BeginFrame));
}

pub fn present() {
    host_call(HostCall::Gpu(GpuCall::Present));
}

pub fn create_buffer(size: u64, usage: BufferUsage, mapped_at_creation: bool) -> GpuId {
    host_call(HostCall::Gpu(GpuCall::CreateBuffer(CreateBuffer {
        size,
        usage,
        mapped_at_creation,
    }))) as GpuId
}

pub fn write_buffer(buffer: GpuId, offset: u64, data: Vec<u8>) {
    host_call(HostCall::Gpu(GpuCall::WriteBuffer(WriteBuffer {
        buffer,
        offset,
        data,
    })));
}

pub fn destroy_buffer(id: GpuId) {
    host_call(HostCall::Gpu(GpuCall::DestroyBuffer { id }));
}

pub fn create_texture(desc: CreateTexture) -> GpuId {
    host_call(HostCall::Gpu(GpuCall::CreateTexture(desc))) as GpuId
}

pub fn create_texture_view(desc: CreateTextureView) -> GpuId {
    host_call(HostCall::Gpu(GpuCall::CreateTextureView(desc))) as GpuId
}

pub fn destroy_texture(id: GpuId) {
    host_call(HostCall::Gpu(GpuCall::DestroyTexture { id }));
}

pub fn create_shader_module(wgsl_source: String) -> GpuId {
    host_call(HostCall::Gpu(GpuCall::CreateShaderModule(
        CreateShaderModule { wgsl_source },
    ))) as GpuId
}

pub fn create_bind_group_layout(desc: CreateBindGroupLayout) -> GpuId {
    host_call(HostCall::Gpu(GpuCall::CreateBindGroupLayout(desc))) as GpuId
}

pub fn create_bind_group(desc: CreateBindGroup) -> GpuId {
    host_call(HostCall::Gpu(GpuCall::CreateBindGroup(desc))) as GpuId
}

pub fn create_pipeline_layout(desc: CreatePipelineLayout) -> GpuId {
    host_call(HostCall::Gpu(GpuCall::CreatePipelineLayout(desc))) as GpuId
}

pub fn create_render_pipeline(desc: CreateRenderPipeline) -> GpuId {
    host_call(HostCall::Gpu(GpuCall::CreateRenderPipeline(desc))) as GpuId
}

pub fn create_compute_pipeline(desc: CreateComputePipeline) -> GpuId {
    host_call(HostCall::Gpu(GpuCall::CreateComputePipeline(desc))) as GpuId
}

pub fn create_command_encoder() -> GpuId {
    host_call(HostCall::Gpu(GpuCall::CreateCommandEncoder)) as GpuId
}

pub fn submit(encoder: GpuId) {
    host_call(HostCall::Gpu(GpuCall::Submit { encoder }));
}

pub fn begin_render_pass(desc: BeginRenderPass) {
    host_call(HostCall::Gpu(GpuCall::BeginRenderPass(desc)));
}

pub fn end_render_pass(pass: GpuId) {
    host_call(HostCall::Gpu(GpuCall::EndRenderPass { pass }));
}

pub fn set_render_pipeline(pass: GpuId, pipeline: GpuId) {
    host_call(HostCall::Gpu(GpuCall::SetRenderPipeline { pass, pipeline }));
}

pub fn set_bind_group(pass: GpuId, index: u32, bind_group: GpuId) {
    host_call(HostCall::Gpu(GpuCall::SetBindGroup {
        pass,
        index,
        bind_group,
    }));
}

pub fn set_vertex_buffer(pass: GpuId, buffer: GpuId, offset: u64, slot: u32, size: Option<u64>) {
    host_call(HostCall::Gpu(GpuCall::SetVertexBuffer(SetVertexBuffer {
        pass,
        buffer,
        offset,
        slot,
        size,
    })));
}

pub fn set_index_buffer(
    pass: GpuId,
    buffer: GpuId,
    offset: u64,
    format: IndexFormat,
    size: Option<u64>,
) {
    host_call(HostCall::Gpu(GpuCall::SetIndexBuffer(SetIndexBuffer {
        pass,
        buffer,
        offset,
        index_format: format,
        size,
    })));
}

pub fn draw(pass: GpuId, vertices: u32, instances: u32) {
    host_call(HostCall::Gpu(GpuCall::Draw(Draw {
        pass,
        vertices,
        instances,
        first_vertex: 0,
        first_instance: 0,
    })));
}

pub fn draw_indexed(pass: GpuId, indices: u32, instances: u32) {
    host_call(HostCall::Gpu(GpuCall::DrawIndexed(DrawIndexed {
        pass,
        indices,
        instances,
        first_index: 0,
        base_vertex: 0,
        first_instance: 0,
    })));
}

// compute pass

pub fn begin_compute_pass(encoder: GpuId) {
    host_call(HostCall::Gpu(GpuCall::BeginComputePass { encoder }));
}

pub fn end_compute_pass(pass: GpuId) {
    host_call(HostCall::Gpu(GpuCall::EndComputePass { pass }));
}

pub fn set_compute_pipeline(pass: GpuId, pipeline: GpuId) {
    host_call(HostCall::Gpu(GpuCall::SetComputePipeline {
        pass,
        pipeline,
    }));
}

pub fn dispatch(pass: GpuId, x: u32, y: u32, z: u32) {
    host_call(HostCall::Gpu(GpuCall::Dispatch { pass, x, y, z }));
}

// Copies

pub fn copy_buffer_to_buffer(cmd: CopyBufferToBuffer) {
    host_call(HostCall::Gpu(GpuCall::CopyBufferToBuffer(cmd)));
}

pub fn copy_buffer_to_texture(cmd: CopyBufferToTexture) {
    host_call(HostCall::Gpu(GpuCall::CopyBufferToTexture(cmd)));
}

pub fn copy_texture_to_buffer(cmd: CopyTextureToBuffer) {
    host_call(HostCall::Gpu(GpuCall::CopyTextureToBuffer(cmd)));
}

pub struct Buffer(pub GpuId);
pub struct RenderPipeline(pub GpuId);
pub struct BindGroup(pub GpuId);
pub struct Texture(pub GpuId);

pub struct Gpu;

impl Gpu {
    pub fn begin_frame(&self) {
        begin_frame();
    }

    pub fn present(&self) {
        present();
    }

    pub fn create_buffer(&self, size: u64, usage: BufferUsage, mapped_at_creation: bool) -> GpuId {
        create_buffer(size, usage, mapped_at_creation)
    }

    pub fn write_buffer(&self, buffer: GpuId, offset: u64, data: Vec<u8>) {
        write_buffer(buffer, offset, data)
    }

    pub fn destroy_buffer(&self, id: GpuId) {
        destroy_buffer(id)
    }

    pub fn create_texture(&self, desc: CreateTexture) -> GpuId {
        create_texture(desc)
    }

    pub fn create_texture_view(&self, desc: CreateTextureView) -> GpuId {
        create_texture_view(desc)
    }

    pub fn destroy_texture(&self, id: GpuId) {
        destroy_texture(id)
    }

    pub fn create_shader_module(&self, wgsl_source: String) -> GpuId {
        create_shader_module(wgsl_source)
    }

    pub fn create_bind_group_layout(&self, desc: CreateBindGroupLayout) -> GpuId {
        create_bind_group_layout(desc)
    }

    pub fn create_bind_group(&self, desc: CreateBindGroup) -> GpuId {
        create_bind_group(desc)
    }

    pub fn create_pipeline_layout(&self, desc: CreatePipelineLayout) -> GpuId {
        create_pipeline_layout(desc)
    }

    pub fn create_render_pipeline(&self, desc: CreateRenderPipeline) -> GpuId {
        host_call(HostCall::Gpu(GpuCall::CreateRenderPipeline(desc))) as GpuId
    }

    pub fn create_compute_pipeline(&self, desc: CreateComputePipeline) -> GpuId {
        create_compute_pipeline(desc)
    }

    pub fn create_command_encoder(&self) -> GpuId {
        create_command_encoder()
    }

    pub fn submit(&self, encoder: GpuId) {
        submit(encoder)
    }

    pub fn begin_render_pass(&self, desc: BeginRenderPass) {
        begin_render_pass(desc)
    }

    pub fn end_render_pass(&self, pass: GpuId) {
        end_render_pass(pass)
    }

    pub fn set_render_pipeline(&self, pass: GpuId, pipeline: GpuId) {
        set_render_pipeline(pass, pipeline)
    }

    pub fn set_bind_group(&self, pass: GpuId, index: u32, bind_group: GpuId) {
        set_bind_group(pass, index, bind_group)
    }

    pub fn set_vertex_buffer(
        &self,
        pass: GpuId,
        buffer: GpuId,
        offset: u64,
        slot: u32,
        size: Option<u64>,
    ) {
        set_vertex_buffer(pass, buffer, offset, slot, size)
    }

    pub fn set_index_buffer(
        &self,
        pass: GpuId,
        buffer: GpuId,
        offset: u64,
        format: IndexFormat,
        size: Option<u64>,
    ) {
        set_index_buffer(pass, buffer, offset, format, size)
    }

    pub fn draw(&self, pass: GpuId, vertices: u32, instances: u32) {
        draw(pass, vertices, instances)
    }

    pub fn draw_indexed(&self, pass: GpuId, indices: u32, instances: u32) {
        draw_indexed(pass, indices, instances)
    }

    // compute pass

    pub fn begin_compute_pass(&self, encoder: GpuId) {
        begin_compute_pass(encoder)
    }

    pub fn end_compute_pass(&self, pass: GpuId) {
        end_compute_pass(pass)
    }

    pub fn set_compute_pipeline(&self, pass: GpuId, pipeline: GpuId) {
        set_compute_pipeline(pass, pipeline)
    }

    pub fn dispatch(&self, pass: GpuId, x: u32, y: u32, z: u32) {
        dispatch(pass, x, y, z)
    }

    // Copies

    pub fn copy_buffer_to_buffer(&self, cmd: CopyBufferToBuffer) {
        copy_buffer_to_buffer(cmd)
    }

    pub fn copy_buffer_to_texture(&self, cmd: CopyBufferToTexture) {
        copy_buffer_to_texture(cmd)
    }

    pub fn copy_texture_to_buffer(&self, cmd: CopyTextureToBuffer) {
        copy_texture_to_buffer(cmd)
    }
}
