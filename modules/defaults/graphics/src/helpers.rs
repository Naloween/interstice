use interstice_sdk::*;

use crate::tables::{
    Draw2DCommand, HasComputeCommandEditHandle, HasDraw2DCommandEditHandle, HasLayerEditHandle,
    HasRenderPassCommandEditHandle, Layer,
};

pub(crate) fn owns_layer(ctx: &ReducerContext, layer: &Layer) -> bool {
    layer.owner_module_name == ctx.caller_node_id
}

pub(crate) fn ensure_layer_exists(ctx: &ReducerContext, name: &str) -> bool {
    if ctx.current.tables.layer().get(name.to_string()).is_none() {
        ctx.log(&format!("Layer '{}' not found", name));
        return false;
    }
    true
}

pub(crate) fn purge_layer_draws(ctx: &ReducerContext, layer: &str) {
    if let Ok(rows) = ctx.current.tables.draw2dcommand().scan() {
        for row in rows.into_iter().filter(|row| row.layer == layer) {
            let _ = ctx.current.tables.draw2dcommand().delete(row.id);
        }
    }
}

pub(crate) fn enqueue_draw_command(ctx: &ReducerContext, command: Draw2DCommand) {
    if let Err(err) = ctx.current.tables.draw2dcommand().insert(command) {
        ctx.log(&format!("Failed to store draw command: {}", err));
    }
}

pub(crate) fn namespaced_key(ctx: &ReducerContext, local_id: &str) -> (String, String) {
    (ctx.caller_node_id.clone(), local_id.to_string())
}

pub(crate) fn clear_commands_tables(ctx: &ReducerContext) {
    if let Ok(rows) = ctx.current.tables.draw2dcommand().scan() {
        for row in rows {
            let _ = ctx.current.tables.draw2dcommand().delete(row.id);
        }
    }
    if let Ok(rows) = ctx.current.tables.renderpasscommand().scan() {
        for row in rows {
            let _ = ctx.current.tables.renderpasscommand().delete(row.id);
        }
    }
    if let Ok(rows) = ctx.current.tables.computecommand().scan() {
        for row in rows {
            let _ = ctx.current.tables.computecommand().delete(row.id);
        }
    }
}
