use interstice_sdk::*;
use std::str::FromStr;

#[interstice_type]
#[derive(Debug, Clone)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

#[interstice_type]
#[derive(Debug, Clone)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[interstice_type]
#[derive(Debug, Clone)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

#[interstice_type]
#[derive(Debug, Clone)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[interstice_type]
#[derive(Debug, Clone)]
pub struct ResourceAddress {
    pub owner_node_id: String,
    pub local_id: String,
}

#[interstice_type]
#[derive(Debug, Clone)]
pub struct CircleCommand {
    pub center: Vec2,
    pub radius: f32,
    pub color: Color,
    pub filled: bool,
    pub stroke_width: f32,
}

#[interstice_type]
#[derive(Debug, Clone)]
pub struct PolylineCommand {
    pub points: Vec<Vec2>,
    pub color: Color,
    pub width: f32,
    pub closed: bool,
    pub filled: bool,
}

#[interstice_type]
#[derive(Debug, Clone)]
pub struct ImageCommand {
    pub texture: ResourceAddress,
    pub rect: Rect,
    pub tint: Color,
}

#[interstice_type]
#[derive(Debug, Clone)]
pub struct TextCommand {
    pub content: String,
    pub position: Vec2,
    pub size: f32,
    pub color: Color,
    pub font: Option<String>,
}

#[interstice_type]
#[derive(Debug, Clone)]
pub struct MeshVertex {
    pub position: Vec3,
    pub color: Color,
    pub uv: Vec2,
}

#[interstice_type]
#[derive(Debug, Clone)]
pub struct MeshDescriptor {
    pub vertices: Vec<MeshVertex>,
    pub indices: Option<Vec<u32>>,
}

#[interstice_type]
#[derive(Debug, Clone)]
pub struct PipelineDescriptorInput {
    pub label: Option<String>,
    pub shader_source: String,
    pub vertex_entry: String,
    pub fragment_entry: Option<String>,
}

#[interstice_type]
#[derive(Debug, Clone)]
pub struct BindGroupDescriptorInput {
    pub label: Option<String>,
}

#[interstice_type]
#[derive(Debug, Clone)]
pub struct TextureUsageFlags {
    pub copy_src: bool,
    pub copy_dst: bool,
    pub texture_binding: bool,
    pub storage_binding: bool,
    pub render_attachment: bool,
}

#[interstice_type]
#[derive(Debug, Clone)]
pub struct TextureDescriptorInput {
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub mip_levels: u32,
    pub sample_count: u32,
    pub usage: TextureUsageFlags,
}

#[interstice_type]
#[derive(Debug, Clone)]
pub struct RenderPassSubmission {
    pub layer: String,
    pub debug_label: Option<String>,
}

#[interstice_type]
#[derive(Debug, Clone)]
pub struct ComputeSubmission {
    pub debug_label: Option<String>,
}

pub(crate) fn color_to_array(color: &Color) -> [f32; 4] {
    [color.r, color.g, color.b, color.a]
}
