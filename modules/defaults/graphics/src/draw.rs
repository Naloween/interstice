use std::str::FromStr;

use interstice_sdk::*;

use crate::helpers::{enqueue_draw_command, ensure_layer_exists, namespaced_key};
use crate::tables::{
    ComputeCommand, Draw2DCommand, HasComputeCommandEditHandle, HasMeshBindingEditHandle,
    HasPipelineBindingEditHandle, HasRenderPassCommandEditHandle, HasTextureBindingEditHandle,
    RenderPassCommand,
};
use crate::types::{
    CircleCommand, Color, ComputeSubmission, ImageCommand, MeshDrawCommand, PolylineCommand, Rect,
    RectCommand, RenderPassSubmission, ResourceAddress, TextCommand, Vec2,
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
        rect: None,
        image: None,
        text: None,
        mesh: None,
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
        rect: None,
        image: None,
        text: None,
        mesh: None,
    };
    enqueue_draw_command(&ctx, command);
}

#[reducer]
pub fn draw_rect(
    ctx: ReducerContext,
    layer: String,
    rect: Rect,
    color: Color,
    filled: bool,
    stroke_width: f32,
) {
    if !ensure_layer_exists(&ctx, &layer) {
        return;
    }
    if rect.w <= 0.0 || rect.h <= 0.0 {
        ctx.log("Rect width and height must be positive");
        return;
    }
    let command = Draw2DCommand {
        id: 0,
        layer,
        command_type: "rect".into(),
        circle: None,
        polyline: None,
        rect: Some(RectCommand {
            rect,
            color,
            filled,
            stroke_width,
        }),
        image: None,
        text: None,
        mesh: None,
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
        rect: None,
        image: Some(ImageCommand {
            texture: ResourceAddress {
                owner_node_id: key.0.clone(),
                local_id: key.1.clone(),
            },
            rect,
            tint,
        }),
        text: None,
        mesh: None,
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
    if size <= 0.0 {
        ctx.log("Text size must be positive");
        return;
    }
    let command = Draw2DCommand {
        id: 0,
        layer,
        command_type: "text".into(),
        circle: None,
        polyline: None,
        rect: None,
        image: None,
        text: Some(TextCommand {
            content,
            position,
            size,
            color,
            font,
        }),
        mesh: None,
    };
    enqueue_draw_command(&ctx, command);
}

#[reducer]
pub fn draw_mesh(
    ctx: ReducerContext,
    layer: String,
    mesh_local_id: String,
    pipeline_local_id: String,
    instances: u32,
) {
    if !ensure_layer_exists(&ctx, &layer) {
        return;
    }

    let mesh_key = namespaced_key(&ctx, &mesh_local_id);
    if ctx
        .current
        .tables
        .meshbinding()
        .get(mesh_key.clone())
        .is_none()
    {
        ctx.log(&format!("Mesh '{}' not found for draw_mesh", mesh_local_id));
        return;
    }

    let pipeline_key = namespaced_key(&ctx, &pipeline_local_id);
    let pipeline = match ctx
        .current
        .tables
        .pipelinebinding()
        .get(pipeline_key.clone())
    {
        Some(value) => value,
        None => {
            ctx.log(&format!(
                "Pipeline '{}' not found for draw_mesh",
                pipeline_local_id
            ));
            return;
        }
    };
    if pipeline.pipeline_id.is_none() {
        ctx.log(&format!(
            "Pipeline '{}' is not compiled yet",
            pipeline_local_id
        ));
        return;
    }

    let command = Draw2DCommand {
        id: 0,
        layer,
        command_type: "mesh".into(),
        circle: None,
        polyline: None,
        rect: None,
        image: None,
        text: None,
        mesh: Some(MeshDrawCommand {
            mesh: ResourceAddress {
                owner_node_id: mesh_key.0,
                local_id: mesh_key.1,
            },
            pipeline: ResourceAddress {
                owner_node_id: pipeline_key.0,
                local_id: pipeline_key.1,
            },
            instances: instances.max(1),
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
