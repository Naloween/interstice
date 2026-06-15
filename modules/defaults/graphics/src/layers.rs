use std::str::FromStr;

use interstice_sdk::*;

use crate::helpers::{layer_key, owns_layer, purge_layer_draws};
use crate::tables::{Draw2DCommand, HasLayerEditHandle, Layer};

#[reducer]
pub fn create_layer<Caps>(ctx: ReducerContext<Caps>, name: String, z: i32, clear: bool)
where
    Caps: CanInsert<Layer> + CanRead<Layer>,
{
    if name.trim().is_empty() {
        ctx.log("Layer name cannot be empty");
        return;
    }

    let key = layer_key(&ctx, &name);
    if ctx.current.tables.layer().get(key.clone()).is_some() {
        ctx.log(&format!("Layer '{}' already exists", name));
        return;
    }

    let layer = Layer {
        name: key,
        z,
        clear,
        owner_module_name: ctx.caller_module_name.clone(),
    };

    if let Err(err) = ctx.current.tables.layer().insert(layer) {
        ctx.log(&format!("Failed to insert layer: {}", err));
    }
}

#[reducer]
pub fn set_layer_z<Caps>(ctx: ReducerContext<Caps>, name: String, z: i32)
where
    Caps: CanRead<Layer> + CanUpdate<Layer>,
{
    match ctx.current.tables.layer().get(layer_key(&ctx, &name)) {
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
    Caps: CanRead<Layer> + CanUpdate<Layer>,
{
    match ctx.current.tables.layer().get(layer_key(&ctx, &name)) {
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
    Caps: CanRead<Layer> + CanDelete<Layer> + CanRead<Draw2DCommand> + CanDelete<Draw2DCommand>,
{
    let key = layer_key(&ctx, &name);
    match ctx.current.tables.layer().get(key.clone()) {
        Some(layer) => {
            if !owns_layer(&ctx, &layer) {
                ctx.log(&format!(
                    "Layer '{}' cannot be deleted by this caller",
                    name
                ));
                return;
            }
            if let Err(err) = ctx.current.tables.layer().delete(key.clone()) {
                ctx.log(&format!("Failed to delete layer: {}", err));
            }
            purge_layer_draws(&ctx, &key);
        }
        None => ctx.log(&format!("Layer '{}' not found", name)),
    }
}
