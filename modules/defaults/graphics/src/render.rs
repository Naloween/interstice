use interstice_sdk::*;
use interstice_sdk::{
    BeginRenderPass, BufferUsage, ColorTargetState, ColorWrites, CreatePipelineLayout,
    CreateRenderPipeline, CreateTextureView, FragmentState, FrontFace, Gpu, LoadOp,
    MultisampleState, PrimitiveState, PrimitiveTopology, RenderPassColorAttachment, StoreOp,
    TextureFormat, TextureViewDimension, VertexAttribute, VertexBufferLayout, VertexFormat,
    VertexState, VertexStepMode,
};
use std::collections::HashMap;

use crate::helpers::clear_ephemeral_tables;
use crate::tables::{
    Draw2DCommand, HasComputeCommandEditHandle, HasDraw2DCommandEditHandle, HasLayerEditHandle,
    HasRenderPassCommandEditHandle, HasRendererCacheEditHandle, RendererCache,
};
use crate::types::{CircleCommand, Color, PolylineCommand, Vec2, color_to_array};
use crate::{DEFAULT_CLEAR, DEFAULT_SEGMENTS, GpuExt, MIN_SEGMENTS, RENDERER_CACHE_KEY};

#[reducer(on = "render")]
pub fn render(ctx: ReducerContext) {
    if let Err(err) = render_inner(&ctx) {
        ctx.log(&format!("Render failed: {}", err));
    }
    clear_ephemeral_tables(&ctx);
}

fn render_inner(ctx: &ReducerContext) -> Result<(), String> {
    let gpu = ctx.gpu();

    gpu.begin_frame()?;

    let (surface_width, surface_height) = gpu.get_surface_size()?;
    let surface = SurfaceInfo::new(surface_width, surface_height);

    let surface_texture = gpu.get_current_surface_texture()?;
    let surface_format = gpu.get_surface_format()?;
    let surface_view_id = gpu.create_texture_view(CreateTextureView {
        texture: surface_texture,
        format: Some(surface_format),
        dimension: Some(TextureViewDimension::D2),
        base_mip_level: 0,
        mip_level_count: None,
        base_array_layer: 0,
        array_layer_count: None,
    })?;
    let surface_view = SurfaceViewGuard::new(ctx.gpu(), surface_view_id);

    let mut layers = ctx.current.tables.layer().scan().unwrap_or_default();
    layers.sort_by_key(|layer| layer.z);

    let draw_rows = ctx
        .current
        .tables
        .draw2dcommand()
        .scan()
        .unwrap_or_default();
    let mut commands_by_layer: HashMap<String, Vec<Draw2DCommand>> = HashMap::new();
    for row in draw_rows {
        commands_by_layer
            .entry(row.layer.clone())
            .or_default()
            .push(row);
    }

    let encoder = gpu.create_command_encoder()?;
    let pipeline = ensure_immediate_pipeline(ctx, &gpu, surface_format)?;
    let mut created_buffers: Vec<u32> = Vec::new();

    if layers.is_empty() {
        record_clear_pass(&gpu, encoder, surface_view.id(), DEFAULT_CLEAR)?;
    } else {
        let mut first_pass = true;
        for layer in layers {
            let layer_commands = commands_by_layer.remove(&layer.name).unwrap_or_default();
            let mut executed_pass = false;

            if layer.clear || first_pass || !layer_commands.is_empty() {
                let mut load = LoadOp::Load;
                let mut clear_color = DEFAULT_CLEAR;
                if first_pass || layer.clear {
                    load = LoadOp::Clear;
                }
                if layer.clear {
                    clear_color = DEFAULT_CLEAR;
                }
                let pass = gpu.begin_render_pass(BeginRenderPass {
                    encoder,
                    color_attachments: vec![RenderPassColorAttachment {
                        view: surface_view.id(),
                        resolve_target: None,
                        load,
                        store: StoreOp::Store,
                        clear_color,
                    }],
                    depth_stencil: None,
                })?;

                if !layer_commands.is_empty() {
                    execute_draw_commands(
                        &gpu,
                        pass,
                        &pipeline,
                        layer_commands,
                        &mut created_buffers,
                        surface,
                    )?;
                }

                gpu.end_render_pass(pass)?;
                executed_pass = true;
            }

            if executed_pass {
                first_pass = false;
            }
        }
    }

    if !commands_by_layer.is_empty() {
        for (layer_name, leftover) in commands_by_layer {
            if !leftover.is_empty() {
                ctx.log(&format!(
                    "Skipped {} draw commands for missing layer '{}'",
                    leftover.len(),
                    layer_name
                ));
            }
        }
    }

    gpu.submit(encoder)?;
    for buffer in created_buffers {
        let _ = gpu.destroy_buffer(buffer);
    }
    gpu.present()?;

    if let Ok(render_passes) = ctx.current.tables.renderpasscommand().scan() {
        if !render_passes.is_empty() {
            ctx.log("Render pass submissions are queued but not executed yet");
        }
    }
    if let Ok(compute_passes) = ctx.current.tables.computecommand().scan() {
        if !compute_passes.is_empty() {
            ctx.log("Compute submissions are queued but not executed yet");
        }
    }

    Ok(())
}

fn record_clear_pass(
    gpu: &Gpu,
    encoder: u32,
    surface_view: u32,
    clear_color: [f32; 4],
) -> Result<(), String> {
    let pass = gpu.begin_render_pass(BeginRenderPass {
        encoder,
        color_attachments: vec![RenderPassColorAttachment {
            view: surface_view,
            resolve_target: None,
            load: LoadOp::Clear,
            store: StoreOp::Store,
            clear_color,
        }],
        depth_stencil: None,
    })?;
    gpu.end_render_pass(pass)
}

fn ensure_immediate_pipeline(
    ctx: &ReducerContext,
    gpu: &Gpu,
    format: TextureFormat,
) -> Result<ImmediatePipeline, String> {
    let mut cache = ctx
        .current
        .tables
        .renderercache()
        .get(RENDERER_CACHE_KEY)
        .unwrap_or(RendererCache {
            id: RENDERER_CACHE_KEY,
            surface_format: None,
            shader_module: None,
            pipeline_layout: None,
            pipeline_id: None,
        });

    let format_label = format_label(format);
    if cache
        .surface_format
        .as_ref()
        .map(|current| current == &format_label)
        .unwrap_or(false)
        && cache.pipeline_id.is_some()
        && cache.shader_module.is_some()
        && cache.pipeline_layout.is_some()
    {
        return Ok(ImmediatePipeline {
            shader: cache.shader_module.unwrap(),
            layout: cache.pipeline_layout.unwrap(),
            pipeline: cache.pipeline_id.unwrap(),
        });
    }

    let shader = gpu.create_shader_module(IMMEDIATE_SHADER.into())?;
    let layout = gpu.create_pipeline_layout(CreatePipelineLayout {
        bind_group_layouts: vec![],
    })?;
    let pipeline = gpu.create_render_pipeline(CreateRenderPipeline {
        label: Some("graphics.defaults.immediate".into()),
        layout,
        vertex: VertexState {
            module: shader,
            entry_point: "vs_main".into(),
            buffers: vec![VertexBufferLayout {
                array_stride: std::mem::size_of::<ImmediateVertexBytes>() as u64,
                step_mode: VertexStepMode::Vertex,
                attributes: vec![
                    VertexAttribute {
                        format: VertexFormat::Float32x2,
                        offset: 0,
                        shader_location: 0,
                    },
                    VertexAttribute {
                        format: VertexFormat::Float32x4,
                        offset: 8,
                        shader_location: 1,
                    },
                ],
            }],
        },
        fragment: Some(FragmentState {
            module: shader,
            entry_point: "fs_main".into(),
            targets: vec![ColorTargetState {
                format,
                blend: None,
                write_mask: ColorWrites::ALL,
            }],
        }),
        primitive: PrimitiveState {
            topology: PrimitiveTopology::TriangleList,
            cull_mode: None,
            front_face: FrontFace::Ccw,
        },
        depth_stencil: None,
        multisample: MultisampleState {
            count: 1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        multiview: None,
    })?;

    cache.surface_format = Some(format_label);
    cache.shader_module = Some(shader);
    cache.pipeline_layout = Some(layout);
    cache.pipeline_id = Some(pipeline);

    if ctx
        .current
        .tables
        .renderercache()
        .get(RENDERER_CACHE_KEY)
        .is_some()
    {
        let _ = ctx.current.tables.renderercache().update(cache.clone());
    } else {
        let _ = ctx.current.tables.renderercache().insert(cache.clone());
    }

    Ok(ImmediatePipeline {
        shader,
        layout,
        pipeline,
    })
}

fn execute_draw_commands(
    gpu: &Gpu,
    pass: u32,
    pipeline: &ImmediatePipeline,
    commands: Vec<Draw2DCommand>,
    created_buffers: &mut Vec<u32>,
    surface: SurfaceInfo,
) -> Result<(), String> {
    gpu.set_render_pipeline(pass, pipeline.pipeline)?;
    for command in commands {
        match command.command_type.as_str() {
            "circle" => {
                if let Some(payload) = command.circle {
                    let vertices = tessellate_circle(surface, &payload);
                    upload_and_draw(gpu, pass, vertices, created_buffers)?;
                }
            }
            "polyline" => {
                if let Some(payload) = command.polyline {
                    let vertices = tessellate_polyline(surface, &payload);
                    upload_and_draw(gpu, pass, vertices, created_buffers)?;
                }
            }
            _ => continue,
        }
    }
    Ok(())
}

fn upload_and_draw(
    gpu: &Gpu,
    pass: u32,
    vertices: Vec<ImmediateVertexBytes>,
    created_buffers: &mut Vec<u32>,
) -> Result<(), String> {
    if vertices.is_empty() {
        return Ok(());
    }
    let data = encode_immediate_vertices(&vertices);
    let buffer = gpu.create_buffer(
        data.len() as u64,
        BufferUsage::VERTEX | BufferUsage::COPY_DST,
        false,
    )?;
    gpu.write_buffer(buffer, 0, data)?;
    gpu.set_vertex_buffer(pass, buffer, 0, 0, None)?;
    gpu.draw(pass, vertices.len() as u32, 1)?;
    created_buffers.push(buffer);
    Ok(())
}

struct ImmediatePipeline {
    shader: u32,
    layout: u32,
    pipeline: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ImmediateVertexBytes {
    position: [f32; 2],
    color: [f32; 4],
}

fn tessellate_circle(surface: SurfaceInfo, cmd: &CircleCommand) -> Vec<ImmediateVertexBytes> {
    let segments = DEFAULT_SEGMENTS.max(MIN_SEGMENTS);
    let mut vertices = Vec::new();
    if cmd.radius <= 0.0 {
        return vertices;
    }
    if cmd.filled {
        for i in 0..segments {
            let angle0 = std::f32::consts::TAU * (i as f32 / segments as f32);
            let angle1 = std::f32::consts::TAU * ((i + 1) as f32 / segments as f32);
            vertices.push(ImmediateVertexBytes {
                position: to_clip(surface, cmd.center.x, cmd.center.y),
                color: color_to_array(&cmd.color),
            });
            vertices.push(ImmediateVertexBytes {
                position: to_clip(
                    surface,
                    cmd.center.x + cmd.radius * angle0.cos(),
                    cmd.center.y + cmd.radius * angle0.sin(),
                ),
                color: color_to_array(&cmd.color),
            });
            vertices.push(ImmediateVertexBytes {
                position: to_clip(
                    surface,
                    cmd.center.x + cmd.radius * angle1.cos(),
                    cmd.center.y + cmd.radius * angle1.sin(),
                ),
                color: color_to_array(&cmd.color),
            });
        }
    } else {
        let half = (cmd.stroke_width.max(0.0001)) * 0.5;
        for i in 0..segments {
            let angle0 = std::f32::consts::TAU * (i as f32 / segments as f32);
            let angle1 = std::f32::consts::TAU * ((i + 1) as f32 / segments as f32);
            let (inner0_x, inner0_y) = polar(cmd, cmd.radius - half, angle0);
            let (outer0_x, outer0_y) = polar(cmd, cmd.radius + half, angle0);
            let (inner1_x, inner1_y) = polar(cmd, cmd.radius - half, angle1);
            let (outer1_x, outer1_y) = polar(cmd, cmd.radius + half, angle1);

            let color = color_to_array(&cmd.color);
            vertices.push(ImmediateVertexBytes {
                position: to_clip(surface, inner0_x, inner0_y),
                color,
            });
            vertices.push(ImmediateVertexBytes {
                position: to_clip(surface, outer0_x, outer0_y),
                color,
            });
            vertices.push(ImmediateVertexBytes {
                position: to_clip(surface, inner1_x, inner1_y),
                color,
            });

            vertices.push(ImmediateVertexBytes {
                position: to_clip(surface, outer0_x, outer0_y),
                color,
            });
            vertices.push(ImmediateVertexBytes {
                position: to_clip(surface, outer1_x, outer1_y),
                color,
            });
            vertices.push(ImmediateVertexBytes {
                position: to_clip(surface, inner1_x, inner1_y),
                color,
            });
        }
    }
    vertices
}

fn tessellate_polyline(surface: SurfaceInfo, cmd: &PolylineCommand) -> Vec<ImmediateVertexBytes> {
    if cmd.points.len() < 2 {
        return Vec::new();
    }
    if cmd.filled {
        return triangulate_polygon(surface, &cmd.points, &cmd.color);
    }
    let mut vertices = Vec::new();
    let mut segments = Vec::new();
    for window in cmd.points.windows(2) {
        segments.push((window[0].clone(), window[1].clone()));
    }
    if cmd.closed {
        if let Some(first) = cmd.points.first().cloned() {
            if let Some(last) = cmd.points.last().cloned() {
                segments.push((last, first));
            }
        }
    }
    let half = cmd.width.max(0.0001) * 0.5;
    for (start, end) in segments {
        let dx = end.x - start.x;
        let dy = end.y - start.y;
        let len = (dx * dx + dy * dy).sqrt();
        if len <= 0.0 {
            continue;
        }
        let nx = -dy / len;
        let ny = dx / len;
        let offset_x = nx * half;
        let offset_y = ny * half;
        let p0 = to_clip(surface, start.x - offset_x, start.y - offset_y);
        let p1 = to_clip(surface, start.x + offset_x, start.y + offset_y);
        let p2 = to_clip(surface, end.x - offset_x, end.y - offset_y);
        let p3 = to_clip(surface, end.x + offset_x, end.y + offset_y);
        let color = color_to_array(&cmd.color);
        vertices.push(ImmediateVertexBytes {
            position: p0,
            color,
        });
        vertices.push(ImmediateVertexBytes {
            position: p2,
            color,
        });
        vertices.push(ImmediateVertexBytes {
            position: p1,
            color,
        });
        vertices.push(ImmediateVertexBytes {
            position: p1,
            color,
        });
        vertices.push(ImmediateVertexBytes {
            position: p2,
            color,
        });
        vertices.push(ImmediateVertexBytes {
            position: p3,
            color,
        });
    }
    vertices
}

fn triangulate_polygon(
    surface: SurfaceInfo,
    points: &[Vec2],
    color: &Color,
) -> Vec<ImmediateVertexBytes> {
    if points.len() < 3 {
        return Vec::new();
    }
    let mut vertices = Vec::new();
    let color = color_to_array(color);
    let origin = points[0].clone();
    for i in 1..points.len() - 1 {
        vertices.push(ImmediateVertexBytes {
            position: to_clip(surface, origin.x, origin.y),
            color,
        });
        vertices.push(ImmediateVertexBytes {
            position: to_clip(surface, points[i].x, points[i].y),
            color,
        });
        vertices.push(ImmediateVertexBytes {
            position: to_clip(surface, points[i + 1].x, points[i + 1].y),
            color,
        });
    }
    vertices
}

fn polar(cmd: &CircleCommand, radius: f32, angle: f32) -> (f32, f32) {
    (
        cmd.center.x + radius.max(0.0) * angle.cos(),
        cmd.center.y + radius.max(0.0) * angle.sin(),
    )
}

fn to_clip(surface: SurfaceInfo, x: f32, y: f32) -> [f32; 2] {
    [
        (x / surface.width) * 2.0 - 1.0,
        1.0 - (y / surface.height) * 2.0,
    ]
}

fn encode_immediate_vertices(vertices: &[ImmediateVertexBytes]) -> Vec<u8> {
    let mut bytes =
        Vec::with_capacity(vertices.len() * std::mem::size_of::<ImmediateVertexBytes>());
    for vertex in vertices {
        bytes.extend_from_slice(&vertex.position[0].to_le_bytes());
        bytes.extend_from_slice(&vertex.position[1].to_le_bytes());
        bytes.extend_from_slice(&vertex.color[0].to_le_bytes());
        bytes.extend_from_slice(&vertex.color[1].to_le_bytes());
        bytes.extend_from_slice(&vertex.color[2].to_le_bytes());
        bytes.extend_from_slice(&vertex.color[3].to_le_bytes());
    }
    bytes
}

fn format_label(format: TextureFormat) -> String {
    match format {
        TextureFormat::Bgra8Unorm => "bgra8unorm".into(),
        TextureFormat::Bgra8UnormSrgb => "bgra8unorm_srgb".into(),
        TextureFormat::Rgba8Unorm => "rgba8unorm".into(),
        TextureFormat::Rgba8UnormSrgb => "rgba8unorm_srgb".into(),
        TextureFormat::Depth24Plus => "depth24plus".into(),
        TextureFormat::Depth32Float => "depth32float".into(),
    }
}

#[derive(Clone, Copy)]
struct SurfaceInfo {
    width: f32,
    height: f32,
}

impl SurfaceInfo {
    fn new(width: u32, height: u32) -> Self {
        Self {
            width: width.max(1) as f32,
            height: height.max(1) as f32,
        }
    }
}

struct SurfaceViewGuard {
    gpu: Gpu,
    view: Option<u32>,
}

impl SurfaceViewGuard {
    fn new(gpu: Gpu, view: u32) -> Self {
        Self {
            gpu,
            view: Some(view),
        }
    }

    fn id(&self) -> u32 {
        self.view.expect("surface view missing")
    }
}

impl Drop for SurfaceViewGuard {
    fn drop(&mut self) {
        if let Some(view) = self.view.take() {
            let _ = self.gpu.destroy_texture_view(view);
        }
    }
}

const IMMEDIATE_SHADER: &str = r#"
struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
) -> VertexOut {
    var out: VertexOut;
    out.position = vec4(position, 0.0, 1.0);
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    return in.color;
}
"#;
