use interstice_sdk::*;
use std::str::FromStr;

use crate::types::{
    CircleCommand, ComputeSubmission, ImageCommand, PolylineCommand, RenderPassSubmission,
    TextCommand,
};

#[table(stateful)]
#[derive(Debug, Clone)]
pub struct Layer {
    #[primary_key]
    pub name: String,
    pub z: i32,
    pub clear: bool,
    pub owner_node_id: String,
}

#[table(stateful)]
#[derive(Debug, Clone)]
pub struct TextureBinding {
    #[primary_key]
    pub key: (String, String),
    pub gpu_id: u32,
    pub width: u32,
    pub height: u32,
    pub format: String,
}

#[table(stateful)]
#[derive(Debug, Clone)]
pub struct MeshBinding {
    #[primary_key]
    pub key: (String, String),
    pub vertex_buffer: u32,
    pub vertex_count: u32,
    pub vertex_stride: u32,
    pub index_buffer: Option<u32>,
    pub index_count: u32,
}

#[table(stateful)]
#[derive(Debug, Clone)]
pub struct PipelineBinding {
    #[primary_key]
    pub key: (String, String),
    pub descriptor: crate::types::PipelineDescriptorInput,
    pub pipeline_id: Option<u32>,
}

#[table(stateful)]
#[derive(Debug, Clone)]
pub struct BindGroupBinding {
    #[primary_key]
    pub key: (String, String),
    pub descriptor: crate::types::BindGroupDescriptorInput,
    pub bind_group_id: Option<u32>,
}

#[table(stateful)]
#[derive(Debug, Clone)]
pub struct RendererCache {
    #[primary_key]
    pub id: u32,
    pub surface_format: Option<String>,
    pub shader_module: Option<u32>,
    pub pipeline_layout: Option<u32>,
    pub pipeline_id: Option<u32>,
}

#[table(ephemeral)]
#[derive(Debug, Clone)]
pub struct Draw2DCommand {
    #[primary_key(auto_inc)]
    pub id: u64,
    pub layer: String,
    pub command_type: String,
    pub circle: Option<CircleCommand>,
    pub polyline: Option<PolylineCommand>,
    pub image: Option<ImageCommand>,
    pub text: Option<TextCommand>,
}

#[table(ephemeral)]
#[derive(Debug, Clone)]
pub struct RenderPassCommand {
    #[primary_key(auto_inc)]
    pub id: u64,
    pub payload: RenderPassSubmission,
}

#[table(ephemeral)]
#[derive(Debug, Clone)]
pub struct ComputeCommand {
    #[primary_key(auto_inc)]
    pub id: u64,
    pub payload: ComputeSubmission,
}
