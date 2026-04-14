use interstice_sdk::*;

use crate::tables::{
    BindGroupBinding, ComputeCommand, Draw2DCommand, FrameTick, HasComputeCommandEditHandle,
    HasDraw2DCommandEditHandle, HasLayerEditHandle, HasRenderPassCommandEditHandle, Layer,
    MeshBinding, PipelineBinding, RenderPassCommand, RendererCache, TextureBinding,
};

pub(crate) fn owns_layer<Caps>(ctx: &ReducerContext<Caps>, layer: &Layer) -> bool {
    layer.owner_module_name == ctx.caller_node_id
}

pub(crate) fn ensure_layer_exists<Caps: CanRead<Layer>>(
    ctx: &ReducerContext<Caps>,
    name: &str,
) -> bool {
    if ctx.current.tables.layer().get(name.to_string()).is_none() {
        ctx.log(&format!("Layer '{}' not found", name));
        return false;
    }
    true
}

pub(crate) fn purge_layer_draws<Caps>(ctx: &ReducerContext<Caps>, layer: &str)
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
    for row in ctx
        .current
        .tables
        .draw2dcommand()
        .scan()
        .into_iter()
        .filter(|row| row.layer == layer)
    {
        let _ = ctx.current.tables.draw2dcommand().delete(row.id);
    }
}

pub(crate) fn enqueue_draw_command<Caps>(ctx: &ReducerContext<Caps>, command: Draw2DCommand)
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
    if let Err(err) = ctx.current.tables.draw2dcommand().insert(command) {
        ctx.log(&format!("Failed to store draw command: {}", err));
    }
}

pub(crate) fn namespaced_key<Caps>(ctx: &ReducerContext<Caps>, local_id: &str) -> (String, String) {
    (ctx.caller_node_id.clone(), local_id.to_string())
}

pub(crate) fn clear_commands_tables<Caps>(ctx: &ReducerContext<Caps>)
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
    let _ = ctx.current.tables.draw2dcommand().clear();
    let _ = ctx.current.tables.renderpasscommand().clear();
    let _ = ctx.current.tables.computecommand().clear();
}
