use std::str::FromStr;

use interstice_sdk::*;
use interstice_sdk::{BufferUsage, CopyBufferToTexture, CreateTexture, TextureDimension, TextureFormat, TextureUsage};

use crate::helpers::namespaced_key;
use crate::tables::{
    BindGroupBinding,
    HasBindGroupBindingEditHandle,
    HasMeshBindingEditHandle,
    HasPipelineBindingEditHandle,
    HasTextureBindingEditHandle,
    MeshBinding,
    PipelineBinding,
    TextureBinding,
};
use crate::types::{
    BindGroupDescriptorInput,
    MeshDescriptor,
    MeshVertex,
    PipelineDescriptorInput,
    TextureDescriptorInput,
    TextureUsageFlags,
};
use crate::GpuExt;

#[reducer]
pub fn create_texture(
    ctx: ReducerContext,
    local_id: String,
    desc: TextureDescriptorInput,
    bytes: Vec<u8>,
) {
    if desc.width == 0 || desc.height == 0 {
        ctx.log("Texture dimensions must be greater than zero");
        return;
    }

    let key = namespaced_key(&ctx, &local_id);
    if ctx
        .current
        .tables
        .texturebinding()
        .get(key.clone())
        .is_some()
    {
        ctx.log(&format!(
            "Texture '{}' already exists for this caller",
            local_id
        ));
        return;
    }

    match allocate_texture(&ctx, &desc, &bytes) {
        Ok(texture_id) => {
            let row = TextureBinding {
                key,
                gpu_id: texture_id,
                width: desc.width,
                height: desc.height,
                format: desc.format.clone(),
            };
            if let Err(err) = ctx.current.tables.texturebinding().insert(row) {
                ctx.log(&format!("Failed to record texture binding: {}", err));
            }
        }
        Err(err) => ctx.log(&format!("Texture allocation failed: {}", err)),
    }
}

#[reducer]
pub fn create_mesh(ctx: ReducerContext, local_id: String, mesh: MeshDescriptor) {
    if mesh.vertices.is_empty() {
        ctx.log("Mesh must include at least one vertex");
        return;
    }

    let key = namespaced_key(&ctx, &local_id);
    if ctx.current.tables.meshbinding().get(key.clone()).is_some() {
        ctx.log(&format!("Mesh '{}' already exists", local_id));
        return;
    }

    match allocate_mesh(&ctx, key.clone(), &mesh) {
        Ok(binding) => {
            if let Err(err) = ctx.current.tables.meshbinding().insert(binding) {
                ctx.log(&format!("Failed to record mesh binding: {}", err));
            }
        }
        Err(err) => ctx.log(&format!("Mesh allocation failed: {}", err)),
    }
}

#[reducer]
pub fn create_pipeline(ctx: ReducerContext, local_id: String, descriptor: PipelineDescriptorInput) {
    let key = namespaced_key(&ctx, &local_id);
    if ctx
        .current
        .tables
        .pipelinebinding()
        .get(key.clone())
        .is_some()
    {
        ctx.log(&format!("Pipeline '{}' already exists", local_id));
        return;
    }
    let row = PipelineBinding {
        key,
        descriptor,
        pipeline_id: None,
    };
    if let Err(err) = ctx.current.tables.pipelinebinding().insert(row) {
        ctx.log(&format!("Failed to store pipeline descriptor: {}", err));
    } else {
        ctx.log("Stored pipeline descriptor (GPU compilation not implemented yet)");
    }
}

#[reducer]
pub fn create_bind_group(
    ctx: ReducerContext,
    local_id: String,
    descriptor: BindGroupDescriptorInput,
) {
    let key = namespaced_key(&ctx, &local_id);
    if ctx
        .current
        .tables
        .bindgroupbinding()
        .get(key.clone())
        .is_some()
    {
        ctx.log(&format!("Bind group '{}' already exists", local_id));
        return;
    }
    let row = BindGroupBinding {
        key,
        descriptor,
        bind_group_id: None,
    };
    if let Err(err) = ctx.current.tables.bindgroupbinding().insert(row) {
        ctx.log(&format!("Failed to store bind group descriptor: {}", err));
    } else {
        ctx.log("Stored bind group descriptor (GPU allocation not implemented yet)");
    }
}

#[reducer]
pub fn destroy_resource(ctx: ReducerContext, local_id: String) {
    let key = namespaced_key(&ctx, &local_id);
    if try_destroy_texture(&ctx, &key).is_some() {
        return;
    }
    if try_destroy_mesh(&ctx, &key).is_some() {
        return;
    }
    if try_destroy_pipeline(&ctx, &key).is_some() {
        return;
    }
    if try_destroy_bind_group(&ctx, &key).is_some() {
        return;
    }
    ctx.log(&format!(
        "No resource named '{}' found for caller",
        local_id
    ));
}

fn allocate_texture(
    ctx: &ReducerContext,
    desc: &TextureDescriptorInput,
    bytes: &[u8],
) -> Result<u32, String> {
    let gpu = ctx.gpu();

    let format = parse_texture_format(&desc.format)?;
    let usage = texture_usage_flags(&desc.usage);

    let texture = gpu.create_texture(CreateTexture {
        width: desc.width,
        height: desc.height,
        depth: 1,
        mip_levels: desc.mip_levels.max(1),
        sample_count: desc.sample_count.max(1),
        dimension: TextureDimension::D2,
        format,
        usage,
    })?;

    if !bytes.is_empty() {
        let expected_len =
            (desc.width as usize) * (desc.height as usize) * bytes_per_pixel(format) as usize;
        if bytes.len() != expected_len {
            return Err(format!(
                "Texture data size mismatch (expected {} bytes, got {})",
                expected_len,
                bytes.len()
            ));
        }
        let staging = gpu.create_buffer(bytes.len() as u64, BufferUsage::COPY_SRC, false)?;
        gpu.write_buffer(staging, 0, bytes.to_vec())?;
        let encoder = gpu.create_command_encoder()?;
        gpu.copy_buffer_to_texture(CopyBufferToTexture {
            encoder,
            src_buffer: staging,
            src_offset: 0,
            bytes_per_row: desc.width * bytes_per_pixel(format),
            rows_per_image: desc.height,
            dst_texture: texture,
            mip_level: 0,
            origin: [0, 0, 0],
            extent: [desc.width, desc.height, 1],
        })?;
        gpu.submit(encoder)?;
        let _ = gpu.destroy_buffer(staging);
    }

    Ok(texture)
}

fn allocate_mesh(
    ctx: &ReducerContext,
    key: (String, String),
    mesh: &MeshDescriptor,
) -> Result<MeshBinding, String> {
    let gpu = ctx.gpu();
    let stride = std::mem::size_of::<MeshVertexBytes>() as u32;
    let vertex_bytes = encode_mesh_vertices(&mesh.vertices);

    let vertex_buffer = gpu.create_buffer(
        vertex_bytes.len() as u64,
        BufferUsage::VERTEX | BufferUsage::COPY_DST,
        false,
    )?;
    gpu.write_buffer(vertex_buffer, 0, vertex_bytes)?;

    let (index_buffer, index_count) = if let Some(indices) = &mesh.indices {
        if indices.is_empty() {
            (None, 0)
        } else {
            let mut data = Vec::with_capacity(indices.len() * 4);
            for value in indices {
                data.extend_from_slice(&value.to_le_bytes());
            }
            let buffer = gpu.create_buffer(
                data.len() as u64,
                BufferUsage::INDEX | BufferUsage::COPY_DST,
                false,
            )?;
            gpu.write_buffer(buffer, 0, data)?;
            (Some(buffer), indices.len() as u32)
        }
    } else {
        (None, 0)
    };

    let binding = MeshBinding {
        key,
        vertex_buffer,
        vertex_count: mesh.vertices.len() as u32,
        vertex_stride: stride,
        index_buffer,
        index_count,
    };

    Ok(binding)
}

fn try_destroy_texture(ctx: &ReducerContext, key: &(String, String)) -> Option<()> {
    let row = ctx.current.tables.texturebinding().get(key.clone())?;
    if let Err(err) = ctx.current.tables.texturebinding().delete(key.clone()) {
        ctx.log(&format!("Failed to delete texture binding: {}", err));
    }
    let gpu = ctx.gpu();
    if let Err(err) = gpu.destroy_texture(row.gpu_id) {
        ctx.log(&format!("Failed to destroy GPU texture: {}", err));
    }
    Some(())
}

fn try_destroy_mesh(ctx: &ReducerContext, key: &(String, String)) -> Option<()> {
    let row = ctx.current.tables.meshbinding().get(key.clone())?;
    if let Err(err) = ctx.current.tables.meshbinding().delete(key.clone()) {
        ctx.log(&format!("Failed to delete mesh binding: {}", err));
    }
    let gpu = ctx.gpu();
    if let Err(err) = gpu.destroy_buffer(row.vertex_buffer) {
        ctx.log(&format!("Failed to destroy vertex buffer: {}", err));
    }
    if let Some(index) = row.index_buffer {
        if let Err(err) = gpu.destroy_buffer(index) {
            ctx.log(&format!("Failed to destroy index buffer: {}", err));
        }
    }
    Some(())
}

fn try_destroy_pipeline(ctx: &ReducerContext, key: &(String, String)) -> Option<()> {
    let row = ctx.current.tables.pipelinebinding().get(key.clone())?;
    if let Err(err) = ctx.current.tables.pipelinebinding().delete(key.clone()) {
        ctx.log(&format!("Failed to delete pipeline descriptor: {}", err));
    }
    if row.pipeline_id.is_some() {
        ctx.log("Destroying GPU pipelines is not implemented yet");
    }
    Some(())
}

fn try_destroy_bind_group(ctx: &ReducerContext, key: &(String, String)) -> Option<()> {
    let row = ctx.current.tables.bindgroupbinding().get(key.clone())?;
    if let Err(err) = ctx.current.tables.bindgroupbinding().delete(key.clone()) {
        ctx.log(&format!("Failed to delete bind group descriptor: {}", err));
    }
    if row.bind_group_id.is_some() {
        ctx.log("Destroying GPU bind groups is not implemented yet");
    }
    Some(())
}

fn texture_usage_flags(flags: &TextureUsageFlags) -> TextureUsage {
    let mut usage = TextureUsage::empty();
    if flags.copy_src {
        usage |= TextureUsage::COPY_SRC;
    }
    if flags.copy_dst {
        usage |= TextureUsage::COPY_DST;
    }
    if flags.texture_binding {
        usage |= TextureUsage::TEXTURE_BINDING;
    }
    if flags.storage_binding {
        usage |= TextureUsage::STORAGE_BINDING;
    }
    if flags.render_attachment {
        usage |= TextureUsage::RENDER_ATTACHMENT;
    }
    usage
}

fn bytes_per_pixel(format: TextureFormat) -> u32 {
    match format {
        TextureFormat::Bgra8Unorm
        | TextureFormat::Bgra8UnormSrgb
        | TextureFormat::Rgba8Unorm
        | TextureFormat::Rgba8UnormSrgb => 4,
        TextureFormat::Depth24Plus | TextureFormat::Depth32Float => 4,
    }
}

fn parse_texture_format(label: &str) -> Result<TextureFormat, String> {
    match label {
        "bgra8unorm" => Ok(TextureFormat::Bgra8Unorm),
        "bgra8unorm_srgb" => Ok(TextureFormat::Bgra8UnormSrgb),
        "rgba8unorm" => Ok(TextureFormat::Rgba8Unorm),
        "rgba8unorm_srgb" => Ok(TextureFormat::Rgba8UnormSrgb),
        "depth24plus" => Ok(TextureFormat::Depth24Plus),
        "depth32float" => Ok(TextureFormat::Depth32Float),
        other => Err(format!("Unsupported texture format '{}'", other)),
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct MeshVertexBytes {
    position: [f32; 3],
    color: [f32; 4],
    uv: [f32; 2],
}

fn encode_mesh_vertices(vertices: &[MeshVertex]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vertices.len() * std::mem::size_of::<MeshVertexBytes>());
    for vertex in vertices {
        let packed = MeshVertexBytes {
            position: [vertex.position.x, vertex.position.y, vertex.position.z],
            color: crate::types::color_to_array(&vertex.color),
            uv: [vertex.uv.x, vertex.uv.y],
        };
        bytes.extend_from_slice(&packed.position[0].to_le_bytes());
        bytes.extend_from_slice(&packed.position[1].to_le_bytes());
        bytes.extend_from_slice(&packed.position[2].to_le_bytes());
        bytes.extend_from_slice(&packed.color[0].to_le_bytes());
        bytes.extend_from_slice(&packed.color[1].to_le_bytes());
        bytes.extend_from_slice(&packed.color[2].to_le_bytes());
        bytes.extend_from_slice(&packed.color[3].to_le_bytes());
        bytes.extend_from_slice(&packed.uv[0].to_le_bytes());
        bytes.extend_from_slice(&packed.uv[1].to_le_bytes());
    }
    bytes
}
