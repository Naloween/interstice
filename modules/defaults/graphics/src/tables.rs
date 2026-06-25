use interstice_sdk::*;
use std::str::FromStr;

use crate::types::{
    BindGroupDescriptorInput, CircleCommand, ComputeSubmission, Draw2DCommandType, ImageCommand,
    MeshDrawCommand, PipelineDescriptorInput, PolylineCommand, RectCommand, RenderPassSubmission,
    SurfaceCommand, TextCommand,
};

#[table(ephemeral)]
#[derive(Debug)]
pub struct Layer {
    #[primary_key]
    pub name: String,
    pub z: i32,
    pub clear: bool,
    pub owner_module_name: String,
}

#[table(ephemeral)]
#[derive(Debug)]
pub struct TextureBinding {
    #[primary_key]
    pub key: (String, String),
    pub gpu_id: u32,
    pub width: u32,
    pub height: u32,
    pub format: String,
}

#[table(ephemeral)]
#[derive(Debug)]
pub struct MeshBinding {
    #[primary_key]
    pub key: (String, String),
    pub vertex_buffer: u32,
    pub vertex_count: u32,
    pub vertex_stride: u32,
    pub index_buffer: Option<u32>,
    pub index_count: u32,
}

#[table(ephemeral)]
#[derive(Debug)]
pub struct PipelineBinding {
    #[primary_key]
    pub key: (String, String),
    pub descriptor: PipelineDescriptorInput,
    pub shader_module_id: Option<u32>,
    pub pipeline_layout_id: Option<u32>,
    pub pipeline_id: Option<u32>,
}

#[table(ephemeral)]
#[derive(Debug)]
pub struct BindGroupBinding {
    #[primary_key]
    pub key: (String, String),
    pub descriptor: BindGroupDescriptorInput,
    pub bind_group_id: Option<u32>,
}

#[table(public, ephemeral)]
#[derive(Debug)]
pub struct FrameTick {
    #[primary_key]
    pub id: u32,
    pub frame: u64,
}

#[table(public, ephemeral)]
#[derive(Debug)]
pub struct SurfaceInfo {
    #[primary_key]
    pub id: u32,
    pub width: u32,
    pub height: u32,
}

/// Render-target bookkeeping for offscreen surfaces (id >= 1). Surface 0 is the
/// swapchain and never has a row here — it is targeted directly each frame.
/// `texture_id`/`view_id` are created lazily by the render loop and cleared
/// (set to `None`) on resize so the loop reallocates at the new size.
#[table(ephemeral)]
#[derive(Debug)]
pub struct SurfaceTarget {
    #[primary_key]
    pub id: u32,
    pub width: u32,
    pub height: u32,
    pub texture_id: Option<u32>,
    pub view_id: Option<u32>,
}

/// Routes a module's layers into a surface. Absence means the module renders to
/// surface 0 (the swapchain), which is the default for every module.
#[table(ephemeral)]
#[derive(Debug)]
pub struct SurfaceAssignment {
    #[primary_key]
    pub module_name: String,
    pub surface_id: u32,
}

/// Records the single module allowed to manage surfaces (claim-compositor gate).
/// Always a single row at id 0; `module_name` is the owner's caller identity.
#[table(ephemeral)]
#[derive(Debug)]
pub struct Compositor {
    #[primary_key]
    pub id: u32,
    pub module_name: String,
}

#[table(ephemeral)]
#[derive(Debug)]
pub struct RendererCache {
    #[primary_key]
    pub id: u32,
    pub surface_format: Option<String>,
    pub shader_module: Option<u32>,
    pub pipeline_layout: Option<u32>,
    pub pipeline_id: Option<u32>,
    // Textured pipeline used to composite offscreen surfaces (draw_surface).
    pub tex_shader_module: Option<u32>,
    pub tex_pipeline_layout: Option<u32>,
    pub tex_bind_group_layout: Option<u32>,
    pub tex_pipeline_id: Option<u32>,
    pub sampler: Option<u32>,
    // Glyph atlas (format-independent): a single RGBA texture of rasterized
    // DejaVu Sans glyphs, sampled through the textured pipeline to draw text.
    pub glyph_atlas_texture: Option<u32>,
    pub glyph_atlas_view: Option<u32>,
}

#[table(ephemeral)]
#[derive(Debug)]
pub struct Draw2DCommand {
    #[primary_key(auto_inc)]
    pub id: u64,
    pub layer: String,
    pub command_type: Draw2DCommandType,
    pub circle: Option<CircleCommand>,
    pub circles: Option<Vec<CircleCommand>>,
    pub polyline: Option<PolylineCommand>,
    pub rect: Option<RectCommand>,
    pub image: Option<ImageCommand>,
    pub surface: Option<SurfaceCommand>,
    pub text: Option<TextCommand>,
    pub mesh: Option<MeshDrawCommand>,
}

#[table(ephemeral)]
#[derive(Debug)]
pub struct RenderPassCommand {
    #[primary_key(auto_inc)]
    pub id: u64,
    pub payload: RenderPassSubmission,
}

#[table(ephemeral)]
#[derive(Debug)]
pub struct ComputeCommand {
    #[primary_key(auto_inc)]
    pub id: u64,
    pub payload: ComputeSubmission,
}
