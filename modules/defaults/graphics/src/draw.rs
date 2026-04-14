use std::str::FromStr;

use interstice_sdk::*;

use crate::helpers::{enqueue_draw_command, ensure_layer_exists, namespaced_key};
use crate::tables::{
    BindGroupBinding, ComputeCommand, Draw2DCommand, FrameTick, HasComputeCommandEditHandle,
    HasMeshBindingEditHandle, HasPipelineBindingEditHandle, HasRenderPassCommandEditHandle,
    HasTextureBindingEditHandle, Layer, MeshBinding, PipelineBinding, RenderPassCommand,
    RendererCache, TextureBinding,
};
use crate::types::{
    CircleCommand, Color, ComputeSubmission, ImageCommand, MeshDrawCommand, PolylineCommand, Rect,
    RectCommand, RenderPassSubmission, ResourceAddress, TextCommand, Vec2,
};

#[reducer]
pub fn draw_circle<
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
>(
    ctx: ReducerContext<Caps>,
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
        circles: None,
        polyline: None,
        rect: None,
        image: None,
        text: None,
        mesh: None,
    };
    enqueue_draw_command(&ctx, command);
}

#[reducer]
pub fn draw_circles<
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
>(
    ctx: ReducerContext<Caps>,
    layer: String,
    centers: Vec<Vec2>,
    radii: Vec<f32>,
    color: Color,
    filled: bool,
    stroke_width: f32,
) {
    if !ensure_layer_exists(&ctx, &layer) {
        return;
    }
    if centers.len() != radii.len() {
        ctx.log("draw_circles: centers and radii length mismatch");
        return;
    }

    let circles = centers
        .into_iter()
        .zip(radii.into_iter())
        .filter_map(|(center, radius)| {
            if radius <= 0.0 {
                None
            } else {
                Some(CircleCommand {
                    center,
                    radius,
                    color: color.clone(),
                    filled,
                    stroke_width,
                })
            }
        })
        .collect::<Vec<_>>();

    if circles.is_empty() {
        return;
    }

    let command = Draw2DCommand {
        id: 0,
        layer,
        command_type: "circles".into(),
        circle: None,
        circles: Some(circles),
        polyline: None,
        rect: None,
        image: None,
        text: None,
        mesh: None,
    };
    enqueue_draw_command(&ctx, command);
}

#[reducer]
pub fn draw_polyline<
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
>(
    ctx: ReducerContext<Caps>,
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
        circles: None,
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
pub fn draw_rect<
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
>(
    ctx: ReducerContext<Caps>,
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
        circles: None,
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
pub fn draw_image<
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
>(
    ctx: ReducerContext<Caps>,
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
        circles: None,
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
pub fn draw_text<
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
>(
    ctx: ReducerContext<Caps>,
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
        circles: None,
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
pub fn draw_mesh<
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
>(
    ctx: ReducerContext<Caps>,
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
        circles: None,
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
pub fn submit_render_pass<
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
>(
    ctx: ReducerContext<Caps>,
    submission: RenderPassSubmission,
) {
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
pub fn submit_compute<
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
>(
    ctx: ReducerContext<Caps>,
    submission: ComputeSubmission,
) {
    if let Err(err) = ctx.current.tables.computecommand().insert(ComputeCommand {
        id: 0,
        payload: submission,
    }) {
        ctx.log(&format!("Failed to enqueue compute command: {}", err));
    }
}
