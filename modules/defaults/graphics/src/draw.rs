use std::str::FromStr;

use interstice_sdk::*;

use crate::helpers::{enqueue_draw_command, ensure_layer_exists, namespaced_key};
use crate::tables::{
    ComputeCommand,
    Draw2DCommand,
    HasComputeCommandEditHandle,
    HasRenderPassCommandEditHandle,
    HasTextureBindingEditHandle,
    RenderPassCommand,
};
use crate::types::{
    CircleCommand, Color, ComputeSubmission, ImageCommand, PolylineCommand, Rect,
    RenderPassSubmission, ResourceAddress, TextCommand, Vec2,
};

#[reducer]
pub fn draw_circle(
    ctx: ReducerContext,
    layer: String,
    center: Vec2,
    radius: f32,
    color: Color,
    filled: bool,
    stroke_width: f32,
) {
    if !ensure_layer_exists(&ctx, &layer) {
        return;
    }
    if radius <= 0.0 {
        ctx.log("Circle radius must be positive");
        return;
    }
    let command = Draw2DCommand {
        id: 0,
        layer,
        command_type: "circle".into(),
        circle: Some(CircleCommand {
            center,
            radius,
            color,
            filled,
            stroke_width,
        }),
        polyline: None,
        image: None,
        text: None,
    };
    enqueue_draw_command(&ctx, command);
}

#[reducer]
pub fn draw_polyline(
    ctx: ReducerContext,
    layer: String,
    points: Vec<Vec2>,
    width: f32,
    color: Color,
    closed: bool,
    filled: bool,
) {
    if !ensure_layer_exists(&ctx, &layer) {
        return;
    }
    if points.len() < 2 {
        ctx.log("Polyline requires at least two points");
        return;
    }
    let command = Draw2DCommand {
        id: 0,
        layer,
        command_type: "polyline".into(),
        circle: None,
        polyline: Some(PolylineCommand {
            points,
            color,
            width,
            closed,
            filled,
        }),
        image: None,
        text: None,
    };
    enqueue_draw_command(&ctx, command);
}

#[reducer]
pub fn draw_image(
    ctx: ReducerContext,
    layer: String,
    texture_local_id: String,
    rect: Rect,
    tint: Color,
) {
    if !ensure_layer_exists(&ctx, &layer) {
        return;
    }
    let key = namespaced_key(&ctx, &texture_local_id);
    if ctx
        .current
        .tables
        .texturebinding()
        .get(key.clone())
        .is_none()
    {
        ctx.log(&format!(
            "Texture '{}' not found for draw_image",
            texture_local_id
        ));
        return;
    }
    let command = Draw2DCommand {
        id: 0,
        layer,
        command_type: "image".into(),
        circle: None,
        polyline: None,
        image: Some(ImageCommand {
            texture: ResourceAddress {
                owner_node_id: key.0.clone(),
                local_id: key.1.clone(),
            },
            rect,
            tint,
        }),
        text: None,
    };
    enqueue_draw_command(&ctx, command);
}

#[reducer]
pub fn draw_text(
    ctx: ReducerContext,
    layer: String,
    content: String,
    position: Vec2,
    size: f32,
    color: Color,
    font: Option<String>,
) {
    if !ensure_layer_exists(&ctx, &layer) {
        return;
    }
    if content.is_empty() {
        return;
    }
    let command = Draw2DCommand {
        id: 0,
        layer,
        command_type: "text".into(),
        circle: None,
        polyline: None,
        image: None,
        text: Some(TextCommand {
            content,
            position,
            size,
            color,
            font,
        }),
    };
    enqueue_draw_command(&ctx, command);
}

#[reducer]
pub fn submit_render_pass(ctx: ReducerContext, submission: RenderPassSubmission) {
    if !ensure_layer_exists(&ctx, &submission.layer) {
        return;
    }
    if let Err(err) = ctx
        .current
        .tables
        .renderpasscommand()
        .insert(RenderPassCommand {
            id: 0,
            payload: submission,
        })
    {
        ctx.log(&format!("Failed to enqueue render pass: {}", err));
    }
}

#[reducer]
pub fn submit_compute(ctx: ReducerContext, submission: ComputeSubmission) {
    if let Err(err) = ctx.current.tables.computecommand().insert(ComputeCommand {
        id: 0,
        payload: submission,
    }) {
        ctx.log(&format!("Failed to enqueue compute command: {}", err));
    }
}
