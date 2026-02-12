use crate::host_calls::{host_call, unpack};
use interstice_abi::*;

fn unpack_gpu_response(pack: i64) -> Result<GpuResponse, String> {
    let response: GpuResponse = unpack(pack);
    match response {
        GpuResponse::Err(err) => Err(err),
        other => Ok(other),
    }
}

fn expect_gpu_none(response: GpuResponse) -> Result<(), String> {
    match response {
        GpuResponse::None => Ok(()),
        other => Err(format!("Unexpected GPU response: {:?}", other)),
    }
}

fn expect_gpu_i64(response: GpuResponse) -> Result<GpuId, String> {
    match response {
        GpuResponse::I64(value) => Ok(value as GpuId),
        other => Err(format!("Unexpected GPU response: {:?}", other)),
    }
}

fn expect_gpu_texture_format(response: GpuResponse) -> Result<TextureFormat, String> {
    match response {
        GpuResponse::TextureFormat(format) => Ok(format),
        other => Err(format!("Unexpected GPU response: {:?}", other)),
    }
}

pub fn begin_frame() -> Result<(), String> {
    let pack = host_call(HostCall::Gpu(GpuCall::BeginFrame));
    expect_gpu_none(unpack_gpu_response(pack)?)
}

pub fn get_surface_format() -> Result<TextureFormat, String> {
    let pack = host_call(HostCall::Gpu(GpuCall::GetSurfaceFormat));
    expect_gpu_texture_format(unpack_gpu_response(pack)?)
}

pub fn get_current_surface_texture() -> Result<GpuId, String> {
    let pack = host_call(HostCall::Gpu(GpuCall::GetCurrentSurfaceTexture));
    expect_gpu_i64(unpack_gpu_response(pack)?)
}

pub fn present() -> Result<(), String> {
    let pack = host_call(HostCall::Gpu(GpuCall::Present));
    expect_gpu_none(unpack_gpu_response(pack)?)
}

pub fn create_buffer(
    size: u64,
    usage: BufferUsage,
    mapped_at_creation: bool,
) -> Result<GpuId, String> {
    let pack = host_call(HostCall::Gpu(GpuCall::CreateBuffer(CreateBuffer {
        size,
        usage,
        mapped_at_creation,
    })));
    expect_gpu_i64(unpack_gpu_response(pack)?)
}

pub fn write_buffer(buffer: GpuId, offset: u64, data: Vec<u8>) -> Result<(), String> {
    let pack = host_call(HostCall::Gpu(GpuCall::WriteBuffer(WriteBuffer {
        buffer,
        offset,
        data,
    })));
    expect_gpu_none(unpack_gpu_response(pack)?)
}

pub fn destroy_buffer(id: GpuId) -> Result<(), String> {
    let pack = host_call(HostCall::Gpu(GpuCall::DestroyBuffer { id }));
    expect_gpu_none(unpack_gpu_response(pack)?)
}

pub fn create_texture(desc: CreateTexture) -> Result<GpuId, String> {
    let pack = host_call(HostCall::Gpu(GpuCall::CreateTexture(desc)));
    expect_gpu_i64(unpack_gpu_response(pack)?)
}

pub fn create_texture_view(desc: CreateTextureView) -> Result<GpuId, String> {
    let pack = host_call(HostCall::Gpu(GpuCall::CreateTextureView(desc)));
    expect_gpu_i64(unpack_gpu_response(pack)?)
}

pub fn destroy_texture(id: GpuId) -> Result<(), String> {
    let pack = host_call(HostCall::Gpu(GpuCall::DestroyTexture { id }));
    expect_gpu_none(unpack_gpu_response(pack)?)
}

pub fn create_shader_module(wgsl_source: String) -> Result<GpuId, String> {
    let pack = host_call(HostCall::Gpu(GpuCall::CreateShaderModule(
        CreateShaderModule { wgsl_source },
    )));
    expect_gpu_i64(unpack_gpu_response(pack)?)
}

pub fn create_bind_group_layout(desc: CreateBindGroupLayout) -> Result<GpuId, String> {
    let pack = host_call(HostCall::Gpu(GpuCall::CreateBindGroupLayout(desc)));
    expect_gpu_i64(unpack_gpu_response(pack)?)
}

pub fn create_bind_group(desc: CreateBindGroup) -> Result<GpuId, String> {
    let pack = host_call(HostCall::Gpu(GpuCall::CreateBindGroup(desc)));
    expect_gpu_i64(unpack_gpu_response(pack)?)
}

pub fn create_pipeline_layout(desc: CreatePipelineLayout) -> Result<GpuId, String> {
    let pack = host_call(HostCall::Gpu(GpuCall::CreatePipelineLayout(desc)));
    expect_gpu_i64(unpack_gpu_response(pack)?)
}

pub fn create_render_pipeline(desc: CreateRenderPipeline) -> Result<GpuId, String> {
    let pack = host_call(HostCall::Gpu(GpuCall::CreateRenderPipeline(desc)));
    expect_gpu_i64(unpack_gpu_response(pack)?)
}

pub fn create_compute_pipeline(desc: CreateComputePipeline) -> Result<GpuId, String> {
    let pack = host_call(HostCall::Gpu(GpuCall::CreateComputePipeline(desc)));
    expect_gpu_i64(unpack_gpu_response(pack)?)
}

pub fn create_command_encoder() -> Result<GpuId, String> {
    let pack = host_call(HostCall::Gpu(GpuCall::CreateCommandEncoder));
    expect_gpu_i64(unpack_gpu_response(pack)?)
}

pub fn submit(encoder: GpuId) -> Result<(), String> {
    let pack = host_call(HostCall::Gpu(GpuCall::Submit { encoder }));
    expect_gpu_none(unpack_gpu_response(pack)?)
}

pub fn begin_render_pass(desc: BeginRenderPass) -> Result<GpuId, String> {
    let pack = host_call(HostCall::Gpu(GpuCall::BeginRenderPass(desc)));
    expect_gpu_i64(unpack_gpu_response(pack)?)
}

pub fn end_render_pass(pass: GpuId) -> Result<(), String> {
    let pack = host_call(HostCall::Gpu(GpuCall::EndRenderPass { pass }));
    expect_gpu_none(unpack_gpu_response(pack)?)
}

pub fn set_render_pipeline(pass: GpuId, pipeline: GpuId) -> Result<(), String> {
    let pack = host_call(HostCall::Gpu(GpuCall::SetRenderPipeline { pass, pipeline }));
    expect_gpu_none(unpack_gpu_response(pack)?)
}

pub fn set_bind_group(pass: GpuId, index: u32, bind_group: GpuId) -> Result<(), String> {
    let pack = host_call(HostCall::Gpu(GpuCall::SetBindGroup {
        pass,
        index,
        bind_group,
    }));
    expect_gpu_none(unpack_gpu_response(pack)?)
}

pub fn set_vertex_buffer(
    pass: GpuId,
    buffer: GpuId,
    offset: u64,
    slot: u32,
    size: Option<u64>,
) -> Result<(), String> {
    let pack = host_call(HostCall::Gpu(GpuCall::SetVertexBuffer(SetVertexBuffer {
        pass,
        buffer,
        offset,
        slot,
        size,
    })));
    expect_gpu_none(unpack_gpu_response(pack)?)
}

pub fn set_index_buffer(
    pass: GpuId,
    buffer: GpuId,
    offset: u64,
    format: IndexFormat,
    size: Option<u64>,
) -> Result<(), String> {
    let pack = host_call(HostCall::Gpu(GpuCall::SetIndexBuffer(SetIndexBuffer {
        pass,
        buffer,
        offset,
        index_format: format,
        size,
    })));
    expect_gpu_none(unpack_gpu_response(pack)?)
}

pub fn draw(pass: GpuId, vertices: u32, instances: u32) -> Result<(), String> {
    let pack = host_call(HostCall::Gpu(GpuCall::Draw(Draw {
        pass,
        vertices,
        instances,
        first_vertex: 0,
        first_instance: 0,
    })));
    expect_gpu_none(unpack_gpu_response(pack)?)
}

pub fn draw_indexed(pass: GpuId, indices: u32, instances: u32) -> Result<(), String> {
    let pack = host_call(HostCall::Gpu(GpuCall::DrawIndexed(DrawIndexed {
        pass,
        indices,
        instances,
        first_index: 0,
        base_vertex: 0,
        first_instance: 0,
    })));
    expect_gpu_none(unpack_gpu_response(pack)?)
}

// compute pass

pub fn begin_compute_pass(encoder: GpuId) -> Result<(), String> {
    let pack = host_call(HostCall::Gpu(GpuCall::BeginComputePass { encoder }));
    expect_gpu_none(unpack_gpu_response(pack)?)
}

pub fn end_compute_pass(pass: GpuId) -> Result<(), String> {
    let pack = host_call(HostCall::Gpu(GpuCall::EndComputePass { pass }));
    expect_gpu_none(unpack_gpu_response(pack)?)
}

pub fn set_compute_pipeline(pass: GpuId, pipeline: GpuId) -> Result<(), String> {
    let pack = host_call(HostCall::Gpu(GpuCall::SetComputePipeline {
        pass,
        pipeline,
    }));
    expect_gpu_none(unpack_gpu_response(pack)?)
}

pub fn dispatch(pass: GpuId, x: u32, y: u32, z: u32) -> Result<(), String> {
    let pack = host_call(HostCall::Gpu(GpuCall::Dispatch { pass, x, y, z }));
    expect_gpu_none(unpack_gpu_response(pack)?)
}

// Copies

pub fn copy_buffer_to_buffer(cmd: CopyBufferToBuffer) -> Result<(), String> {
    let pack = host_call(HostCall::Gpu(GpuCall::CopyBufferToBuffer(cmd)));
    expect_gpu_none(unpack_gpu_response(pack)?)
}

pub fn copy_buffer_to_texture(cmd: CopyBufferToTexture) -> Result<(), String> {
    let pack = host_call(HostCall::Gpu(GpuCall::CopyBufferToTexture(cmd)));
    expect_gpu_none(unpack_gpu_response(pack)?)
}

pub fn copy_texture_to_buffer(cmd: CopyTextureToBuffer) -> Result<(), String> {
    let pack = host_call(HostCall::Gpu(GpuCall::CopyTextureToBuffer(cmd)));
    expect_gpu_none(unpack_gpu_response(pack)?)
}

pub struct Buffer(pub GpuId);
pub struct RenderPipeline(pub GpuId);
pub struct BindGroup(pub GpuId);
pub struct Texture(pub GpuId);

pub struct Gpu;

impl Gpu {
    pub fn begin_frame(&self) -> Result<(), String> {
        begin_frame()
    }

    pub fn get_surface_format(&self) -> Result<TextureFormat, String> {
        get_surface_format()
    }

    pub fn get_current_surface_texture(&self) -> Result<GpuId, String> {
        get_current_surface_texture()
    }

    pub fn present(&self) -> Result<(), String> {
        present()
    }

    pub fn create_buffer(
        &self,
        size: u64,
        usage: BufferUsage,
        mapped_at_creation: bool,
    ) -> Result<GpuId, String> {
        create_buffer(size, usage, mapped_at_creation)
    }

    pub fn write_buffer(&self, buffer: GpuId, offset: u64, data: Vec<u8>) -> Result<(), String> {
        write_buffer(buffer, offset, data)
    }

    pub fn destroy_buffer(&self, id: GpuId) -> Result<(), String> {
        destroy_buffer(id)
    }

    pub fn create_texture(&self, desc: CreateTexture) -> Result<GpuId, String> {
        create_texture(desc)
    }

    pub fn create_texture_view(&self, desc: CreateTextureView) -> Result<GpuId, String> {
        create_texture_view(desc)
    }

    pub fn destroy_texture(&self, id: GpuId) -> Result<(), String> {
        destroy_texture(id)
    }

    pub fn create_shader_module(&self, wgsl_source: String) -> Result<GpuId, String> {
        create_shader_module(wgsl_source)
    }

    pub fn create_bind_group_layout(&self, desc: CreateBindGroupLayout) -> Result<GpuId, String> {
        create_bind_group_layout(desc)
    }

    pub fn create_bind_group(&self, desc: CreateBindGroup) -> Result<GpuId, String> {
        create_bind_group(desc)
    }

    pub fn create_pipeline_layout(&self, desc: CreatePipelineLayout) -> Result<GpuId, String> {
        create_pipeline_layout(desc)
    }

    pub fn create_render_pipeline(&self, desc: CreateRenderPipeline) -> Result<GpuId, String> {
        create_render_pipeline(desc)
    }

    pub fn create_compute_pipeline(&self, desc: CreateComputePipeline) -> Result<GpuId, String> {
        create_compute_pipeline(desc)
    }

    pub fn create_command_encoder(&self) -> Result<GpuId, String> {
        create_command_encoder()
    }

    pub fn submit(&self, encoder: GpuId) -> Result<(), String> {
        submit(encoder)
    }

    pub fn begin_render_pass(&self, desc: BeginRenderPass) -> Result<GpuId, String> {
        begin_render_pass(desc)
    }

    pub fn end_render_pass(&self, pass: GpuId) -> Result<(), String> {
        end_render_pass(pass)
    }

    pub fn set_render_pipeline(&self, pass: GpuId, pipeline: GpuId) -> Result<(), String> {
        set_render_pipeline(pass, pipeline)
    }

    pub fn set_bind_group(&self, pass: GpuId, index: u32, bind_group: GpuId) -> Result<(), String> {
        set_bind_group(pass, index, bind_group)
    }

    pub fn set_vertex_buffer(
        &self,
        pass: GpuId,
        buffer: GpuId,
        offset: u64,
        slot: u32,
        size: Option<u64>,
    ) -> Result<(), String> {
        set_vertex_buffer(pass, buffer, offset, slot, size)
    }

    pub fn set_index_buffer(
        &self,
        pass: GpuId,
        buffer: GpuId,
        offset: u64,
        format: IndexFormat,
        size: Option<u64>,
    ) -> Result<(), String> {
        set_index_buffer(pass, buffer, offset, format, size)
    }

    pub fn draw(&self, pass: GpuId, vertices: u32, instances: u32) -> Result<(), String> {
        draw(pass, vertices, instances)
    }

    pub fn draw_indexed(&self, pass: GpuId, indices: u32, instances: u32) -> Result<(), String> {
        draw_indexed(pass, indices, instances)
    }

    // compute pass

    pub fn begin_compute_pass(&self, encoder: GpuId) -> Result<(), String> {
        begin_compute_pass(encoder)
    }

    pub fn end_compute_pass(&self, pass: GpuId) -> Result<(), String> {
        end_compute_pass(pass)
    }

    pub fn set_compute_pipeline(&self, pass: GpuId, pipeline: GpuId) -> Result<(), String> {
        set_compute_pipeline(pass, pipeline)
    }

    pub fn dispatch(&self, pass: GpuId, x: u32, y: u32, z: u32) -> Result<(), String> {
        dispatch(pass, x, y, z)
    }

    // Copies

    pub fn copy_buffer_to_buffer(&self, cmd: CopyBufferToBuffer) -> Result<(), String> {
        copy_buffer_to_buffer(cmd)
    }

    pub fn copy_buffer_to_texture(&self, cmd: CopyBufferToTexture) -> Result<(), String> {
        copy_buffer_to_texture(cmd)
    }

    pub fn copy_texture_to_buffer(&self, cmd: CopyTextureToBuffer) -> Result<(), String> {
        copy_texture_to_buffer(cmd)
    }
}
