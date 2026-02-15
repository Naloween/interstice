use std::str::FromStr;

use interstice_sdk::*;

use crate::helpers::{owns_layer, purge_layer_draws};
use crate::tables::{HasLayerEditHandle, Layer};

#[reducer]
pub fn create_layer(ctx: ReducerContext, name: String, z: i32, clear: bool) {
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
        owner_node_id: ctx.caller_node_id.clone(),
    };

    if let Err(err) = ctx.current.tables.layer().insert(layer) {
        ctx.log(&format!("Failed to insert layer: {}", err));
    }
}

#[reducer]
pub fn set_layer_z(ctx: ReducerContext, name: String, z: i32) {
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
pub fn set_layer_clear(ctx: ReducerContext, name: String, clear: bool) {
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
pub fn destroy_layer(ctx: ReducerContext, name: String) {
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
