use interstice_sdk::*;

use crate::tables::{
    ComputeCommand, Draw2DCommand, HasComputeCommandEditHandle, HasDraw2DCommandEditHandle,
    HasLayerEditHandle, HasRenderPassCommandEditHandle, Layer, RenderPassCommand,
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
    Caps: CanRead<Draw2DCommand> + CanDelete<Draw2DCommand>,
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
    Caps: CanInsert<Draw2DCommand>,
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
    Caps: CanDelete<Draw2DCommand> + CanDelete<RenderPassCommand> + CanDelete<ComputeCommand>,
{
    let _ = ctx.current.tables.draw2dcommand().clear();
    let _ = ctx.current.tables.renderpasscommand().clear();
    let _ = ctx.current.tables.computecommand().clear();
}
