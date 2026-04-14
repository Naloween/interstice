use std::str::FromStr;

use interstice_sdk::*;

use crate::helpers::{owns_layer, purge_layer_draws};
use crate::tables::{
    BindGroupBinding, ComputeCommand, Draw2DCommand, FrameTick, HasLayerEditHandle, Layer,
    MeshBinding, PipelineBinding, RenderPassCommand, RendererCache, TextureBinding,
};

#[reducer]
pub fn create_layer<Caps>(ctx: ReducerContext<Caps>, name: String, z: i32, clear: bool)
where
    Caps: CanRead<Layer>
        + CanInsert<Layer>
        + CanUpdate<Layer>
        + CanDelete<Layer>
        + CanRead<TextureBinding>
        + CanInsert<TextureBinding>
        + CanUpdate<TextureBinding>
        + CanDelete<TextureBinding>
        + CanRead<MeshBinding>
        + CanInsert<MeshBinding>
        + CanUpdate<MeshBinding>
        + CanDelete<MeshBinding>
        + CanRead<PipelineBinding>
        + CanInsert<PipelineBinding>
        + CanUpdate<PipelineBinding>
        + CanDelete<PipelineBinding>
        + CanRead<BindGroupBinding>
        + CanInsert<BindGroupBinding>
        + CanUpdate<BindGroupBinding>
        + CanDelete<BindGroupBinding>
        + CanRead<FrameTick>
        + CanInsert<FrameTick>
        + CanUpdate<FrameTick>
        + CanDelete<FrameTick>
        + CanRead<RendererCache>
        + CanInsert<RendererCache>
        + CanUpdate<RendererCache>
        + CanDelete<RendererCache>
        + CanRead<Draw2DCommand>
        + CanInsert<Draw2DCommand>
        + CanUpdate<Draw2DCommand>
        + CanDelete<Draw2DCommand>
        + CanRead<RenderPassCommand>
        + CanInsert<RenderPassCommand>
        + CanUpdate<RenderPassCommand>
        + CanDelete<RenderPassCommand>
        + CanRead<ComputeCommand>
        + CanInsert<ComputeCommand>
        + CanUpdate<ComputeCommand>
        + CanDelete<ComputeCommand>,
{
    if name.trim().is_empty() {
        ctx.log("Layer name cannot be empty");
        return;
    }

    if ctx.current.tables.layer().get(name.clone()).is_some() {
        ctx.log(&format!("Layer '{}' already exists", name));
        return;
    }

    let layer = Layer {
        name,
        z,
        clear,
        owner_module_name: ctx.caller_node_id.clone(),
    };

    if let Err(err) = ctx.current.tables.layer().insert(layer) {
        ctx.log(&format!("Failed to insert layer: {}", err));
    }
}

#[reducer]
pub fn set_layer_z<Caps>(ctx: ReducerContext<Caps>, name: String, z: i32)
where
    Caps: CanRead<Layer>
        + CanInsert<Layer>
        + CanUpdate<Layer>
        + CanDelete<Layer>
        + CanRead<TextureBinding>
        + CanInsert<TextureBinding>
        + CanUpdate<TextureBinding>
        + CanDelete<TextureBinding>
        + CanRead<MeshBinding>
        + CanInsert<MeshBinding>
        + CanUpdate<MeshBinding>
        + CanDelete<MeshBinding>
        + CanRead<PipelineBinding>
        + CanInsert<PipelineBinding>
        + CanUpdate<PipelineBinding>
        + CanDelete<PipelineBinding>
        + CanRead<BindGroupBinding>
        + CanInsert<BindGroupBinding>
        + CanUpdate<BindGroupBinding>
        + CanDelete<BindGroupBinding>
        + CanRead<FrameTick>
        + CanInsert<FrameTick>
        + CanUpdate<FrameTick>
        + CanDelete<FrameTick>
        + CanRead<RendererCache>
        + CanInsert<RendererCache>
        + CanUpdate<RendererCache>
        + CanDelete<RendererCache>
        + CanRead<Draw2DCommand>
        + CanInsert<Draw2DCommand>
        + CanUpdate<Draw2DCommand>
        + CanDelete<Draw2DCommand>
        + CanRead<RenderPassCommand>
        + CanInsert<RenderPassCommand>
        + CanUpdate<RenderPassCommand>
        + CanDelete<RenderPassCommand>
        + CanRead<ComputeCommand>
        + CanInsert<ComputeCommand>
        + CanUpdate<ComputeCommand>
        + CanDelete<ComputeCommand>,
{
    match ctx.current.tables.layer().get(name.clone()) {
        Some(mut layer) => {
            if !owns_layer(&ctx, &layer) {
                ctx.log(&format!(
                    "Layer '{}' cannot be modified by this caller",
                    name
                ));
                return;
            }
            layer.z = z;
            if let Err(err) = ctx.current.tables.layer().update(layer) {
                ctx.log(&format!("Failed to update layer z: {}", err));
            }
        }
        None => ctx.log(&format!("Layer '{}' not found", name)),
    }
}

#[reducer]
pub fn set_layer_clear<Caps>(ctx: ReducerContext<Caps>, name: String, clear: bool)
where
    Caps: CanRead<Layer>
        + CanInsert<Layer>
        + CanUpdate<Layer>
        + CanDelete<Layer>
        + CanRead<TextureBinding>
        + CanInsert<TextureBinding>
        + CanUpdate<TextureBinding>
        + CanDelete<TextureBinding>
        + CanRead<MeshBinding>
        + CanInsert<MeshBinding>
        + CanUpdate<MeshBinding>
        + CanDelete<MeshBinding>
        + CanRead<PipelineBinding>
        + CanInsert<PipelineBinding>
        + CanUpdate<PipelineBinding>
        + CanDelete<PipelineBinding>
        + CanRead<BindGroupBinding>
        + CanInsert<BindGroupBinding>
        + CanUpdate<BindGroupBinding>
        + CanDelete<BindGroupBinding>
        + CanRead<FrameTick>
        + CanInsert<FrameTick>
        + CanUpdate<FrameTick>
        + CanDelete<FrameTick>
        + CanRead<RendererCache>
        + CanInsert<RendererCache>
        + CanUpdate<RendererCache>
        + CanDelete<RendererCache>
        + CanRead<Draw2DCommand>
        + CanInsert<Draw2DCommand>
        + CanUpdate<Draw2DCommand>
        + CanDelete<Draw2DCommand>
        + CanRead<RenderPassCommand>
        + CanInsert<RenderPassCommand>
        + CanUpdate<RenderPassCommand>
        + CanDelete<RenderPassCommand>
        + CanRead<ComputeCommand>
        + CanInsert<ComputeCommand>
        + CanUpdate<ComputeCommand>
        + CanDelete<ComputeCommand>,
{
    match ctx.current.tables.layer().get(name.clone()) {
        Some(mut layer) => {
            if !owns_layer(&ctx, &layer) {
                ctx.log(&format!(
                    "Layer '{}' cannot be modified by this caller",
                    name
                ));
                return;
            }
            layer.clear = clear;
            if let Err(err) = ctx.current.tables.layer().update(layer) {
                ctx.log(&format!("Failed to update layer clear flag: {}", err));
            }
        }
        None => ctx.log(&format!("Layer '{}' not found", name)),
    }
}

#[reducer]
pub fn destroy_layer<Caps>(ctx: ReducerContext<Caps>, name: String)
where
    Caps: CanRead<Layer>
        + CanInsert<Layer>
        + CanUpdate<Layer>
        + CanDelete<Layer>
        + CanRead<TextureBinding>
        + CanInsert<TextureBinding>
        + CanUpdate<TextureBinding>
        + CanDelete<TextureBinding>
        + CanRead<MeshBinding>
        + CanInsert<MeshBinding>
        + CanUpdate<MeshBinding>
        + CanDelete<MeshBinding>
        + CanRead<PipelineBinding>
        + CanInsert<PipelineBinding>
        + CanUpdate<PipelineBinding>
        + CanDelete<PipelineBinding>
        + CanRead<BindGroupBinding>
        + CanInsert<BindGroupBinding>
        + CanUpdate<BindGroupBinding>
        + CanDelete<BindGroupBinding>
        + CanRead<FrameTick>
        + CanInsert<FrameTick>
        + CanUpdate<FrameTick>
        + CanDelete<FrameTick>
        + CanRead<RendererCache>
        + CanInsert<RendererCache>
        + CanUpdate<RendererCache>
        + CanDelete<RendererCache>
        + CanRead<Draw2DCommand>
        + CanInsert<Draw2DCommand>
        + CanUpdate<Draw2DCommand>
        + CanDelete<Draw2DCommand>
        + CanRead<RenderPassCommand>
        + CanInsert<RenderPassCommand>
        + CanUpdate<RenderPassCommand>
        + CanDelete<RenderPassCommand>
        + CanRead<ComputeCommand>
        + CanInsert<ComputeCommand>
        + CanUpdate<ComputeCommand>
        + CanDelete<ComputeCommand>,
{
    match ctx.current.tables.layer().get(name.clone()) {
        Some(layer) => {
            if !owns_layer(&ctx, &layer) {
                ctx.log(&format!(
                    "Layer '{}' cannot be deleted by this caller",
                    name
                ));
                return;
            }
            if let Err(err) = ctx.current.tables.layer().delete(name.clone()) {
                ctx.log(&format!("Failed to delete layer: {}", err));
            }
            purge_layer_draws(&ctx, &name);
        }
        None => ctx.log(&format!("Layer '{}' not found", name)),
    }
}
