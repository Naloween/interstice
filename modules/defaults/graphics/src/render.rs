use crate::font;
use interstice_sdk::*;
use interstice_sdk::{
    AddressMode, BeginRenderPass, BindGroupEntry, BindGroupLayoutEntry, BindingResource,
    BindingType, BlendComponent, BlendFactor, BlendOperation, BlendState, BufferUsage,
    ColorTargetState, ColorWrites, CreateBindGroup, CreateBindGroupLayout, CreatePipelineLayout,
    CreateRenderPipeline, CreateSampler, CreateTexture, CreateTextureView, FilterMode,
    FragmentState, FrontFace, Gpu, IndexFormat, LoadOp, MultisampleState, PrimitiveState,
    PrimitiveTopology, RenderPassColorAttachment, ShaderStage, StoreOp, TextureDimension,
    TextureFormat, TextureSampleType, TextureUsage, TextureViewDimension, VertexAttribute,
    VertexBufferLayout, VertexFormat, VertexState, VertexStepMode,
};
use std::collections::HashMap;

use crate::helpers::clear_commands_tables;
use crate::tables::{
    ComputeCommand, Draw2DCommand, FrameTick, HasComputeCommandEditHandle,
    HasDraw2DCommandEditHandle, HasFrameTickEditHandle, HasLayerEditHandle,
    HasMeshBindingEditHandle, HasPipelineBindingEditHandle, HasRenderPassCommandEditHandle,
    HasRendererCacheEditHandle, HasSurfaceAssignmentEditHandle, HasSurfaceInfoEditHandle,
    HasSurfaceTargetEditHandle, HasTextureBindingEditHandle, Layer, MeshBinding, PipelineBinding,
    RenderPassCommand, RendererCache, SurfaceAssignment, SurfaceInfo, SurfaceTarget,
    TextureBinding,
};
use crate::surfaces::SWAPCHAIN_SURFACE_ID;
use crate::types::{
    CircleCommand, Color, Draw2DCommandType, ImageCommand, MeshDrawCommand, PolylineCommand, Rect,
    RectCommand, SurfaceCommand, TextCommand, Vec2, color_to_array,
};
use crate::{DEFAULT_CLEAR, DEFAULT_SEGMENTS, GpuExt, MIN_SEGMENTS, RENDERER_CACHE_KEY};

#[reducer(on = "render")]
pub fn render<Caps>(ctx: ReducerContext<Caps>)
where
    Caps: CanRead<Layer>
        + CanRead<Draw2DCommand>
        + CanRead<RendererCache>
        + CanRead<RenderPassCommand>
        + CanRead<ComputeCommand>
        + CanRead<FrameTick>
        + CanUpdate<FrameTick>
        + CanRead<SurfaceInfo>
        + CanUpdate<SurfaceInfo>
        + CanRead<SurfaceTarget>
        + CanUpdate<SurfaceTarget>
        + CanRead<SurfaceAssignment>
        + CanRead<MeshBinding>
        + CanRead<PipelineBinding>
        + CanRead<TextureBinding>
        + CanDelete<Draw2DCommand>
        + CanDelete<RenderPassCommand>
        + CanDelete<ComputeCommand>
        + CanInsert<RendererCache>
        + CanUpdate<RendererCache>
        + CanDelete<RendererCache>,
{
    if let Err(err) = render_inner(&ctx) {
        ctx.log(&format!("Render failed: {}", err));
    }
    bump_frame_tick(&ctx);
    clear_commands_tables(&ctx);
}

fn bump_frame_tick<Caps>(ctx: &ReducerContext<Caps>)
where
    Caps: CanRead<FrameTick> + CanUpdate<FrameTick>,
{
    let mut row = ctx.current.tables.frametick().get(0).unwrap();
    row.frame = row.frame.saturating_add(1);
    let _ = ctx.current.tables.frametick().update(row);
}

fn render_inner<Caps>(ctx: &ReducerContext<Caps>) -> Result<(), String>
where
    Caps: CanRead<Layer>
        + CanRead<Draw2DCommand>
        + CanRead<RendererCache>
        + CanRead<RenderPassCommand>
        + CanRead<ComputeCommand>
        + CanRead<MeshBinding>
        + CanRead<PipelineBinding>
        + CanRead<TextureBinding>
        + CanRead<RendererCache>
        + CanInsert<RendererCache>
        + CanUpdate<RendererCache>
        + CanDelete<RendererCache>
        + CanRead<SurfaceInfo>
        + CanUpdate<SurfaceInfo>
        + CanRead<SurfaceTarget>
        + CanUpdate<SurfaceTarget>
        + CanRead<SurfaceAssignment>,
{
    let gpu = ctx.gpu();

    gpu.begin_frame()?;

    let (surface_width, surface_height) = gpu.get_surface_size()?;

    if let Some(mut info) = ctx.current.tables.surfaceinfo().get(SWAPCHAIN_SURFACE_ID) {
        info.width = surface_width;
        info.height = surface_height;
        let _ = ctx.current.tables.surfaceinfo().update(info);
    }

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

    let mut layers = ctx.current.tables.layer().scan();
    layers.sort_by_key(|layer| layer.z);

    let draw_rows = ctx.current.tables.draw2dcommand().scan();
    let mut commands_by_layer: HashMap<String, Vec<Draw2DCommand>> = HashMap::new();
    for row in draw_rows {
        commands_by_layer
            .entry(row.layer.clone())
            .or_default()
            .push(row);
    }

    // Route each layer to its owner module's assigned surface (default: 0).
    let assignments: HashMap<String, u32> = ctx
        .current
        .tables
        .surfaceassignment()
        .scan()
        .into_iter()
        .map(|a| (a.module_name, a.surface_id))
        .collect();
    let mut layers_by_surface: HashMap<u32, Vec<Layer>> = HashMap::new();
    for layer in layers {
        let surface_id = assignments
            .get(&layer.owner_module_name)
            .copied()
            .unwrap_or(SWAPCHAIN_SURFACE_ID);
        layers_by_surface.entry(surface_id).or_default().push(layer);
    }

    let encoder = gpu.create_command_encoder()?;
    let (pipeline, textured, atlas_view) = ensure_pipelines(ctx, &gpu, surface_format)?;
    let mut created_buffers: Vec<u32> = Vec::new();
    let mut created_bind_groups: Vec<u32> = Vec::new();
    // Texture views created per-frame for draw_image (one per image command).
    // Destroyed after submit alongside buffers/bind-groups so they don't leak.
    let mut created_views: Vec<u32> = Vec::new();

    // Offscreen surfaces (id >= 1) are rendered FIRST so the compositor can
    // sample their populated views when compositing the swapchain in this same
    // frame. Every known offscreen surface is drawn (cleared even when it has no
    // layers this frame). `surface_views` maps each surface id to its view for
    // draw_surface to bind.
    let mut surface_views: HashMap<u32, u32> = HashMap::new();
    for mut target in ctx.current.tables.surfacetarget().scan() {
        let view_id = ensure_surface_view(&gpu, surface_format, &mut target)?;
        let _ = ctx.current.tables.surfacetarget().update(target.clone());
        surface_views.insert(target.id, view_id);

        let target_layers = layers_by_surface.remove(&target.id).unwrap_or_default();
        let target_surface = RenderSurface::new(target.width, target.height);
        render_layers_to_view(
            ctx,
            &gpu,
            encoder,
            view_id,
            target_surface,
            &target_layers,
            &mut commands_by_layer,
            &pipeline,
            &textured,
            atlas_view,
            &surface_views,
            &mut created_buffers,
            &mut created_bind_groups,
            &mut created_views,
        )?;
    }

    // Swapchain surface (id 0): draw its layer group to the presented view,
    // compositing any offscreen surfaces referenced via draw_surface.
    let swapchain_layers = layers_by_surface
        .remove(&SWAPCHAIN_SURFACE_ID)
        .unwrap_or_default();
    let swapchain_surface = RenderSurface::new(surface_width, surface_height);
    render_layers_to_view(
        ctx,
        &gpu,
        encoder,
        surface_view.id(),
        swapchain_surface,
        &swapchain_layers,
        &mut commands_by_layer,
        &pipeline,
        &textured,
        atlas_view,
        &surface_views,
        &mut created_buffers,
        &mut created_bind_groups,
        &mut created_views,
    )?;

    // Any layers routed to a surface that no longer exists are dropped silently;
    // remaining commands belong to layers that were never declared.
    layers_by_surface.clear();
    if !commands_by_layer.is_empty() {
        for (layer_name, leftover) in &commands_by_layer {
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
    for bind_group in created_bind_groups {
        let _ = gpu.destroy_bind_group(bind_group);
    }
    for view in created_views {
        let _ = gpu.destroy_texture_view(view);
    }
    gpu.present()?;
    // Queue next frame so render continues
    let _ = gpu.request_redraw();

    let render_passes = ctx.current.tables.renderpasscommand().scan();
    if !render_passes.is_empty() {
        ctx.log("Render pass submissions are queued but not executed yet");
    }
    let compute_passes = ctx.current.tables.computecommand().scan();
    if !compute_passes.is_empty() {
        ctx.log("Compute submissions are queued but not executed yet");
    }

    Ok(())
}

/// Render an ordered (z-sorted) subset of layers into a single target view,
/// applying the per-layer clear/load logic. Used once per surface (swapchain or
/// offscreen). With no layers, the view is still cleared.
fn render_layers_to_view<Caps>(
    ctx: &ReducerContext<Caps>,
    gpu: &Gpu,
    encoder: u32,
    view_id: u32,
    surface: RenderSurface,
    layers: &[Layer],
    commands_by_layer: &mut HashMap<String, Vec<Draw2DCommand>>,
    pipeline: &ImmediatePipeline,
    textured: &TexturedPipeline,
    glyph_atlas_view: u32,
    surface_views: &HashMap<u32, u32>,
    created_buffers: &mut Vec<u32>,
    created_bind_groups: &mut Vec<u32>,
    created_views: &mut Vec<u32>,
) -> Result<(), String>
where
    Caps: CanRead<Layer>
        + CanRead<MeshBinding>
        + CanRead<PipelineBinding>
        + CanRead<TextureBinding>,
{
    if layers.is_empty() {
        return record_clear_pass(gpu, encoder, view_id, DEFAULT_CLEAR);
    }

    let mut first_pass = true;
    for layer in layers {
        let layer_commands = commands_by_layer.remove(&layer.name).unwrap_or_default();

        if layer.clear || first_pass || !layer_commands.is_empty() {
            let load = if first_pass || layer.clear {
                LoadOp::Clear
            } else {
                LoadOp::Load
            };
            let pass = gpu.begin_render_pass(BeginRenderPass {
                encoder,
                color_attachments: vec![RenderPassColorAttachment {
                    view: view_id,
                    resolve_target: None,
                    load,
                    store: StoreOp::Store,
                    clear_color: DEFAULT_CLEAR,
                }],
                depth_stencil: None,
            })?;

            if !layer_commands.is_empty() {
                execute_draw_commands(
                    ctx,
                    gpu,
                    pass,
                    pipeline,
                    textured,
                    glyph_atlas_view,
                    surface_views,
                    layer_commands,
                    created_buffers,
                    created_bind_groups,
                    created_views,
                    surface,
                )?;
            }

            gpu.end_render_pass(pass)?;
            first_pass = false;
        }
    }

    Ok(())
}

/// Lazily allocate the offscreen texture + view for a surface target. Returns
/// the view id; callers persist the (possibly updated) target row afterwards.
fn ensure_surface_view(
    gpu: &Gpu,
    format: TextureFormat,
    target: &mut SurfaceTarget,
) -> Result<u32, String> {
    if let Some(view) = target.view_id {
        return Ok(view);
    }

    let texture = gpu.create_texture(CreateTexture {
        width: target.width.max(1),
        height: target.height.max(1),
        depth: 1,
        mip_levels: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format,
        usage: TextureUsage::RENDER_ATTACHMENT | TextureUsage::TEXTURE_BINDING,
    })?;
    let view = gpu.create_texture_view(CreateTextureView {
        texture,
        format: Some(format),
        dimension: Some(TextureViewDimension::D2),
        base_mip_level: 0,
        mip_level_count: None,
        base_array_layer: 0,
        array_layer_count: None,
    })?;

    target.texture_id = Some(texture);
    target.view_id = Some(view);
    Ok(view)
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

/// Build (or reuse) both the immediate-mode pipeline and the textured pipeline
/// used to composite offscreen surfaces. The `RendererCache` row is read once
/// and written at most once so that a freshly-inserted row is never re-read
/// within the same reducer transaction (writes only become visible next frame).
/// A surface-format change invalidates and rebuilds both pipelines.
fn ensure_pipelines<Caps>(
    ctx: &ReducerContext<Caps>,
    gpu: &Gpu,
    format: TextureFormat,
) -> Result<(ImmediatePipeline, TexturedPipeline, u32), String>
where
    Caps: CanRead<RendererCache> + CanInsert<RendererCache> + CanUpdate<RendererCache>,
{
    let existed = ctx
        .current
        .tables
        .renderercache()
        .get(RENDERER_CACHE_KEY)
        .is_some();
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
            tex_shader_module: None,
            tex_pipeline_layout: None,
            tex_bind_group_layout: None,
            tex_pipeline_id: None,
            sampler: None,
            glyph_atlas_texture: None,
            glyph_atlas_view: None,
        });

    let format_label = format_label(format);
    let mut dirty = false;

    // Surface format changed: invalidate every cached pipeline so both rebuild.
    if cache.surface_format.as_ref() != Some(&format_label) {
        cache.surface_format = Some(format_label);
        cache.shader_module = None;
        cache.pipeline_layout = None;
        cache.pipeline_id = None;
        cache.tex_shader_module = None;
        cache.tex_pipeline_layout = None;
        cache.tex_bind_group_layout = None;
        cache.tex_pipeline_id = None;
        cache.sampler = None;
        dirty = true;
    }

    // Immediate-mode pipeline.
    if cache.pipeline_id.is_none() {
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
                    blend: Some(BlendState {
                        color: BlendComponent {
                            src_factor: BlendFactor::SrcAlpha,
                            dst_factor: BlendFactor::OneMinusSrcAlpha,
                            operation: BlendOperation::Add,
                        },
                        alpha: BlendComponent {
                            src_factor: BlendFactor::One,
                            dst_factor: BlendFactor::OneMinusSrcAlpha,
                            operation: BlendOperation::Add,
                        },
                    }),
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

        cache.shader_module = Some(shader);
        cache.pipeline_layout = Some(layout);
        cache.pipeline_id = Some(pipeline);
        dirty = true;
    }
    let immediate = ImmediatePipeline {
        pipeline: cache.pipeline_id.unwrap(),
    };

    // Textured pipeline + shared sampler used to composite offscreen surfaces.
    if cache.tex_pipeline_id.is_none()
        || cache.tex_bind_group_layout.is_none()
        || cache.sampler.is_none()
    {
        let shader = gpu.create_shader_module(TEXTURED_SHADER.into())?;
        let bind_group_layout = gpu.create_bind_group_layout(CreateBindGroupLayout {
            entries: vec![
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStage::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStage::FRAGMENT,
                    ty: BindingType::Sampler { comparison: false },
                },
            ],
        })?;
        let layout = gpu.create_pipeline_layout(CreatePipelineLayout {
            bind_group_layouts: vec![bind_group_layout],
        })?;
        let sampler = gpu.create_sampler(CreateSampler {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            address_mode: AddressMode::ClampToEdge,
        })?;
        let pipeline = gpu.create_render_pipeline(CreateRenderPipeline {
            label: Some("graphics.defaults.textured".into()),
            layout,
            vertex: VertexState {
                module: shader,
                entry_point: "vs_main".into(),
                buffers: vec![VertexBufferLayout {
                    array_stride: std::mem::size_of::<[f32; 8]>() as u64,
                    step_mode: VertexStepMode::Vertex,
                    attributes: vec![
                        VertexAttribute {
                            format: VertexFormat::Float32x2,
                            offset: 0,
                            shader_location: 0,
                        },
                        VertexAttribute {
                            format: VertexFormat::Float32x2,
                            offset: 8,
                            shader_location: 1,
                        },
                        VertexAttribute {
                            format: VertexFormat::Float32x4,
                            offset: 16,
                            shader_location: 2,
                        },
                    ],
                }],
            },
            fragment: Some(FragmentState {
                module: shader,
                entry_point: "fs_main".into(),
                targets: vec![ColorTargetState {
                    format,
                    blend: Some(BlendState {
                        color: BlendComponent {
                            src_factor: BlendFactor::SrcAlpha,
                            dst_factor: BlendFactor::OneMinusSrcAlpha,
                            operation: BlendOperation::Add,
                        },
                        alpha: BlendComponent {
                            src_factor: BlendFactor::One,
                            dst_factor: BlendFactor::OneMinusSrcAlpha,
                            operation: BlendOperation::Add,
                        },
                    }),
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

        cache.tex_shader_module = Some(shader);
        cache.tex_pipeline_layout = Some(layout);
        cache.tex_bind_group_layout = Some(bind_group_layout);
        cache.tex_pipeline_id = Some(pipeline);
        cache.sampler = Some(sampler);
        dirty = true;
    }
    let textured = TexturedPipeline {
        pipeline: cache.tex_pipeline_id.unwrap(),
        bind_group_layout: cache.tex_bind_group_layout.unwrap(),
        sampler: cache.sampler.unwrap(),
    };

    // Glyph atlas: format-independent, so it is *not* reset by a surface-format
    // change above — build + upload the rasterized font once and cache its view.
    if cache.glyph_atlas_view.is_none() {
        let atlas = font::atlas();
        let texture = upload_atlas(gpu, atlas)?;
        let view = gpu.create_texture_view(CreateTextureView {
            texture,
            format: Some(TextureFormat::Rgba8Unorm),
            dimension: Some(TextureViewDimension::D2),
            base_mip_level: 0,
            mip_level_count: None,
            base_array_layer: 0,
            array_layer_count: None,
        })?;
        cache.glyph_atlas_texture = Some(texture);
        cache.glyph_atlas_view = Some(view);
        dirty = true;
    }
    let atlas_view = cache.glyph_atlas_view.unwrap();

    if dirty {
        if existed {
            let _ = ctx.current.tables.renderercache().update(cache);
        } else {
            let _ = ctx.current.tables.renderercache().insert(cache);
        }
    }

    Ok((immediate, textured, atlas_view))
}

/// Create an RGBA8 texture from the glyph atlas pixels and upload them. Rows are
/// padded to the 256-byte copy alignment (the atlas width is already a multiple
/// of 64 px so the common case needs no padding). Runs once (cached thereafter).
fn upload_atlas(gpu: &Gpu, atlas: &font::Atlas) -> Result<u32, String> {
    let texture = gpu.create_texture(CreateTexture {
        width: atlas.width,
        height: atlas.height,
        depth: 1,
        mip_levels: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsage::TEXTURE_BINDING | TextureUsage::COPY_DST,
    })?;

    const ALIGN: u32 = 256;
    let unpadded_row = atlas.width * 4;
    let padded_row = unpadded_row.div_ceil(ALIGN) * ALIGN;
    let staged: Vec<u8> = if padded_row == unpadded_row {
        atlas.rgba.clone()
    } else {
        let urow = unpadded_row as usize;
        let prow = padded_row as usize;
        let mut buf = vec![0u8; prow * atlas.height as usize];
        for y in 0..atlas.height as usize {
            buf[y * prow..y * prow + urow].copy_from_slice(&atlas.rgba[y * urow..y * urow + urow]);
        }
        buf
    };

    let staging = gpu.create_buffer(
        staged.len() as u64,
        BufferUsage::COPY_SRC | BufferUsage::COPY_DST,
        false,
    )?;
    gpu.write_buffer(staging, 0, staged)?;
    let encoder = gpu.create_command_encoder()?;
    gpu.copy_buffer_to_texture(CopyBufferToTexture {
        encoder,
        src_buffer: staging,
        src_offset: 0,
        bytes_per_row: padded_row,
        rows_per_image: atlas.height,
        dst_texture: texture,
        mip_level: 0,
        origin: [0, 0, 0],
        extent: [atlas.width, atlas.height, 1],
    })?;
    gpu.submit(encoder)?;
    let _ = gpu.destroy_buffer(staging);

    Ok(texture)
}

fn execute_draw_commands<Caps>(
    ctx: &ReducerContext<Caps>,
    gpu: &Gpu,
    pass: u32,
    immediate_pipeline: &ImmediatePipeline,
    textured: &TexturedPipeline,
    glyph_atlas_view: u32,
    surface_views: &HashMap<u32, u32>,
    commands: Vec<Draw2DCommand>,
    created_buffers: &mut Vec<u32>,
    created_bind_groups: &mut Vec<u32>,
    created_views: &mut Vec<u32>,
    surface: RenderSurface,
) -> Result<(), String>
where
    Caps: CanRead<Layer>
        + CanRead<MeshBinding>
        + CanRead<PipelineBinding>
        + CanRead<TextureBinding>,
{
    // Batch all immediate draw commands (circle, polyline, rect). Text is drawn
    // separately as textured glyph-atlas quads (see `glyph_vertices`).
    let mut immediate_vertices: Vec<ImmediateVertexBytes> = Vec::new();
    let mut glyph_vertices: Vec<TexturedVertex> = Vec::new();

    for command in &commands {
        match command.command_type {
            Draw2DCommandType::Circle => {
                if let Some(payload) = &command.circle {
                    immediate_vertices.extend(tessellate_circle(surface, payload));
                }
            }
            Draw2DCommandType::Circles => {
                if let Some(batch) = &command.circles {
                    for payload in batch {
                        immediate_vertices.extend(tessellate_circle(surface, payload));
                    }
                }
            }
            Draw2DCommandType::Polyline => {
                if let Some(payload) = &command.polyline {
                    immediate_vertices.extend(tessellate_polyline(surface, payload));
                }
            }
            Draw2DCommandType::Rect => {
                if let Some(payload) = &command.rect {
                    immediate_vertices.extend(tessellate_rect(surface, payload));
                }
            }
            Draw2DCommandType::Text => {
                if let Some(payload) = &command.text {
                    tessellate_text(surface, payload, &mut glyph_vertices);
                }
            }
            _ => {}
        }
    }

    // Only one buffer and draw call for all immediate geometry
    if !immediate_vertices.is_empty() {
        gpu.set_render_pipeline(pass, immediate_pipeline.pipeline)?;
        let data = encode_immediate_vertices(&immediate_vertices);
        let buffer = gpu.create_buffer(
            data.len() as u64,
            BufferUsage::VERTEX | BufferUsage::COPY_DST,
            false,
        )?;
        gpu.write_buffer(buffer, 0, data)?;
        gpu.set_vertex_buffer(pass, buffer, 0, 0, None)?;
        gpu.draw(pass, immediate_vertices.len() as u32, 1)?;
        created_buffers.push(buffer);
    }

    // Text: one bind group (glyph atlas + shared sampler) and one draw call for
    // every glyph quad in this layer. Drawn after the immediate batch (so text
    // sits above backgrounds/underlines) but before images (matching the prior
    // immediate-text ordering, where images composite last).
    if !glyph_vertices.is_empty() {
        let bind_group = gpu.create_bind_group(CreateBindGroup {
            layout: textured.bind_group_layout,
            entries: vec![
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(glyph_atlas_view),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(textured.sampler),
                },
            ],
        })?;
        let data = encode_textured_vertices(&glyph_vertices);
        let buffer = gpu.create_buffer(
            data.len() as u64,
            BufferUsage::VERTEX | BufferUsage::COPY_DST,
            false,
        )?;
        gpu.write_buffer(buffer, 0, data)?;
        gpu.set_render_pipeline(pass, textured.pipeline)?;
        gpu.set_bind_group(pass, 0, bind_group)?;
        gpu.set_vertex_buffer(pass, buffer, 0, 0, None)?;
        gpu.draw(pass, glyph_vertices.len() as u32, 1)?;
        created_buffers.push(buffer);
        created_bind_groups.push(bind_group);
    }

    // Mesh commands are still drawn individually
    for command in &commands {
        if command.command_type == Draw2DCommandType::Mesh {
            if let Some(payload) = &command.mesh {
                draw_mesh_command(ctx, gpu, pass, payload)?;
            }
        }
    }

    // Composite offscreen surfaces (draw_surface). Each references an offscreen
    // surface's view by id; a per-draw bind group binds that view + the shared
    // sampler, then a textured quad is drawn into the destination rect.
    for command in &commands {
        if command.command_type == Draw2DCommandType::Surface {
            if let Some(payload) = &command.surface {
                draw_surface_command(
                    gpu,
                    pass,
                    textured,
                    surface_views,
                    payload,
                    surface,
                    created_buffers,
                    created_bind_groups,
                )?;
            }
        }
    }

    // Composite images (draw_image). Each references a TextureBinding by address;
    // a per-frame view of that texture is bound with the shared sampler and drawn
    // as a textured quad into the destination rect.
    for command in &commands {
        if command.command_type == Draw2DCommandType::Image {
            if let Some(payload) = &command.image {
                draw_image_command(
                    ctx,
                    gpu,
                    pass,
                    textured,
                    payload,
                    surface,
                    created_buffers,
                    created_bind_groups,
                    created_views,
                )?;
            }
        }
    }

    Ok(())
}

/// Draw a single offscreen surface into `payload.dest` using the textured
/// pipeline. No-op when the referenced surface has no view yet this frame.
fn draw_surface_command(
    gpu: &Gpu,
    pass: u32,
    textured: &TexturedPipeline,
    surface_views: &HashMap<u32, u32>,
    payload: &SurfaceCommand,
    surface: RenderSurface,
    created_buffers: &mut Vec<u32>,
    created_bind_groups: &mut Vec<u32>,
) -> Result<(), String> {
    let Some(&view_id) = surface_views.get(&payload.surface_id) else {
        // The surface is unknown or has not been rendered yet this frame.
        return Ok(());
    };
    // Surfaces are always composited whole: sample the full texture (0,0,1,1).
    let full_uv = Rect {
        x: 0.0,
        y: 0.0,
        w: 1.0,
        h: 1.0,
    };
    draw_textured_quad(
        gpu,
        pass,
        textured,
        view_id,
        &payload.dest,
        &full_uv,
        &payload.tint,
        surface,
        created_buffers,
        created_bind_groups,
    )
}

/// Draw an image (`draw_image`) into `payload.rect`. Resolves the texture from
/// its `TextureBinding`, creates a per-frame view (tracked for cleanup), and
/// binds it through the same textured pipeline used for surface compositing.
fn draw_image_command<Caps>(
    ctx: &ReducerContext<Caps>,
    gpu: &Gpu,
    pass: u32,
    textured: &TexturedPipeline,
    payload: &ImageCommand,
    surface: RenderSurface,
    created_buffers: &mut Vec<u32>,
    created_bind_groups: &mut Vec<u32>,
    created_views: &mut Vec<u32>,
) -> Result<(), String>
where
    Caps: CanRead<TextureBinding>,
{
    let key = (
        payload.texture.owner_node_id.clone(),
        payload.texture.local_id.clone(),
    );
    let Some(binding) = ctx.current.tables.texturebinding().get(key) else {
        ctx.log("Skipping draw_image: texture binding not found");
        return Ok(());
    };

    // Inherit the texture's own format (None) so we don't need to re-parse it.
    let view = gpu.create_texture_view(CreateTextureView {
        texture: binding.gpu_id,
        format: None,
        dimension: Some(TextureViewDimension::D2),
        base_mip_level: 0,
        mip_level_count: None,
        base_array_layer: 0,
        array_layer_count: None,
    })?;
    created_views.push(view);

    draw_textured_quad(
        gpu,
        pass,
        textured,
        view,
        &payload.rect,
        &payload.src_uv,
        &payload.tint,
        surface,
        created_buffers,
        created_bind_groups,
    )
}

/// Bind `view_id` + the shared sampler and draw a textured quad covering `dest`
/// with `tint`. Shared by surface compositing and image drawing. Created GPU
/// resources are pushed onto the cleanup vecs (destroyed after submit).
#[allow(clippy::too_many_arguments)]
fn draw_textured_quad(
    gpu: &Gpu,
    pass: u32,
    textured: &TexturedPipeline,
    view_id: u32,
    dest: &Rect,
    uv: &Rect,
    tint: &Color,
    surface: RenderSurface,
    created_buffers: &mut Vec<u32>,
    created_bind_groups: &mut Vec<u32>,
) -> Result<(), String> {
    let bind_group = gpu.create_bind_group(CreateBindGroup {
        layout: textured.bind_group_layout,
        entries: vec![
            BindGroupEntry {
                binding: 0,
                resource: BindingResource::TextureView(view_id),
            },
            BindGroupEntry {
                binding: 1,
                resource: BindingResource::Sampler(textured.sampler),
            },
        ],
    })?;

    let tint = color_to_array(tint);
    let vertices = textured_quad(surface, dest, uv, tint);
    let data = encode_textured_vertices(&vertices);
    let buffer = gpu.create_buffer(
        data.len() as u64,
        BufferUsage::VERTEX | BufferUsage::COPY_DST,
        false,
    )?;
    gpu.write_buffer(buffer, 0, data)?;

    gpu.set_render_pipeline(pass, textured.pipeline)?;
    gpu.set_bind_group(pass, 0, bind_group)?;
    gpu.set_vertex_buffer(pass, buffer, 0, 0, None)?;
    gpu.draw(pass, vertices.len() as u32, 1)?;

    created_buffers.push(buffer);
    created_bind_groups.push(bind_group);
    Ok(())
}

fn draw_mesh_command<Caps>(
    ctx: &ReducerContext<Caps>,
    gpu: &Gpu,
    pass: u32,
    payload: &MeshDrawCommand,
) -> Result<(), String>
where
    Caps: CanRead<Layer> + CanRead<MeshBinding> + CanRead<PipelineBinding>,
{
    let mesh_key = (
        payload.mesh.owner_node_id.clone(),
        payload.mesh.local_id.clone(),
    );
    let pipeline_key = (
        payload.pipeline.owner_node_id.clone(),
        payload.pipeline.local_id.clone(),
    );

    let Some(mesh) = ctx.current.tables.meshbinding().get(mesh_key) else {
        ctx.log("Skipping mesh draw: mesh binding not found");
        return Ok(());
    };

    let Some(pipeline_binding) = ctx.current.tables.pipelinebinding().get(pipeline_key) else {
        ctx.log("Skipping mesh draw: pipeline binding not found");
        return Ok(());
    };

    let Some(pipeline_id) = pipeline_binding.pipeline_id else {
        ctx.log("Skipping mesh draw: pipeline was not compiled");
        return Ok(());
    };

    gpu.set_render_pipeline(pass, pipeline_id)?;
    gpu.set_vertex_buffer(pass, mesh.vertex_buffer, 0, 0, None)?;

    let instances = payload.instances.max(1);
    if let Some(index_buffer) = mesh.index_buffer {
        gpu.set_index_buffer(pass, index_buffer, 0, IndexFormat::Uint32, None)?;
        gpu.draw_indexed(pass, mesh.index_count, instances)?;
    } else {
        gpu.draw(pass, mesh.vertex_count, instances)?;
    }

    Ok(())
}

struct ImmediatePipeline {
    pipeline: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ImmediateVertexBytes {
    position: [f32; 2],
    color: [f32; 4],
}

struct TexturedPipeline {
    pipeline: u32,
    bind_group_layout: u32,
    sampler: u32,
}

#[derive(Clone, Copy)]
struct TexturedVertex {
    position: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}

/// Build the two triangles (6 vertices) of a textured quad covering `dest`
/// (pixel coordinates in the destination surface) with full 0..1 UVs.
fn textured_quad(
    surface: RenderSurface,
    dest: &Rect,
    uv: &Rect,
    color: [f32; 4],
) -> Vec<TexturedVertex> {
    let (x0, y0) = (dest.x, dest.y);
    let (x1, y1) = (dest.x + dest.w, dest.y + dest.h);
    let (u0, v0) = (uv.x, uv.y);
    let (u1, v1) = (uv.x + uv.w, uv.y + uv.h);
    let corner = |x: f32, y: f32, u: f32, v: f32| TexturedVertex {
        position: to_clip(surface, x, y),
        uv: [u, v],
        color,
    };
    vec![
        corner(x0, y0, u0, v0),
        corner(x1, y0, u1, v0),
        corner(x0, y1, u0, v1),
        corner(x0, y1, u0, v1),
        corner(x1, y0, u1, v0),
        corner(x1, y1, u1, v1),
    ]
}

fn encode_textured_vertices(vertices: &[TexturedVertex]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vertices.len() * std::mem::size_of::<[f32; 8]>());
    for v in vertices {
        for f in v.position.iter().chain(v.uv.iter()).chain(v.color.iter()) {
            bytes.extend_from_slice(&f.to_le_bytes());
        }
    }
    bytes
}

fn tessellate_circle(surface: RenderSurface, cmd: &CircleCommand) -> Vec<ImmediateVertexBytes> {
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

fn tessellate_polyline(surface: RenderSurface, cmd: &PolylineCommand) -> Vec<ImmediateVertexBytes> {
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
    let half = cmd.width as f32 * 0.5;
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
    surface: RenderSurface,
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

fn tessellate_rect(surface: RenderSurface, cmd: &RectCommand) -> Vec<ImmediateVertexBytes> {
    if cmd.rect.w <= 0.0 || cmd.rect.h <= 0.0 {
        return Vec::new();
    }

    if let Some(r) = cmd.corner_radius {
        if r > 0.0 {
            return tessellate_rounded_rect(surface, cmd, r);
        }
    }

    if cmd.filled {
        let color = color_to_array(&cmd.color);
        let x0 = cmd.rect.x;
        let y0 = cmd.rect.y;
        let x1 = x0 + cmd.rect.w;
        let y1 = y0 + cmd.rect.h;

        return vec![
            ImmediateVertexBytes { position: to_clip(surface, x0, y0), color },
            ImmediateVertexBytes { position: to_clip(surface, x1, y0), color },
            ImmediateVertexBytes { position: to_clip(surface, x0, y1), color },
            ImmediateVertexBytes { position: to_clip(surface, x0, y1), color },
            ImmediateVertexBytes { position: to_clip(surface, x1, y0), color },
            ImmediateVertexBytes { position: to_clip(surface, x1, y1), color },
        ];
    }

    let x0 = cmd.rect.x;
    let y0 = cmd.rect.y;
    let x1 = x0 + cmd.rect.w;
    let y1 = y0 + cmd.rect.h;

    tessellate_polyline(
        surface,
        &PolylineCommand {
            points: vec![
                Vec2 { x: x0, y: y0 },
                Vec2 { x: x1, y: y0 },
                Vec2 { x: x1, y: y1 },
                Vec2 { x: x0, y: y1 },
            ],
            color: cmd.color.clone(),
            width: cmd.stroke_width as u32,
            closed: true,
            filled: false,
        },
    )
}

/// Generates the outline polygon of a rounded rectangle going clockwise in screen-space.
/// Each corner is approximated by `CORNER_SEGS` arc segments.
fn rounded_rect_outline(rect: &crate::types::Rect, r: f32) -> Vec<Vec2> {
    let x = rect.x;
    let y = rect.y;
    let w = rect.w;
    let h = rect.h;
    let r = r.min(w * 0.5).min(h * 0.5).max(0.0);

    const CORNER_SEGS: usize = 8;
    let mut pts = Vec::with_capacity((CORNER_SEGS + 1) * 4);

    // Corners in clockwise order (screen-space, y-down):
    // Each entry: (center_x, center_y, angle_start, angle_end)
    // angle 0 = right, π/2 = down, π = left, 3π/2 = up
    let half_pi = std::f32::consts::FRAC_PI_2;
    let corners = [
        (x + w - r, y + r,     -half_pi, 0.0),            // top-right
        (x + w - r, y + h - r,  0.0,     half_pi),         // bottom-right
        (x + r,     y + h - r,  half_pi, std::f32::consts::PI), // bottom-left
        (x + r,     y + r,      std::f32::consts::PI, 3.0 * half_pi), // top-left
    ];

    for (cx, cy, a0, a1) in corners {
        for i in 0..=CORNER_SEGS {
            let t = i as f32 / CORNER_SEGS as f32;
            let angle: f32 = a0 + (a1 - a0) * t;
            pts.push(Vec2 {
                x: cx + r * angle.cos(),
                y: cy + r * angle.sin(),
            });
        }
    }

    pts
}

fn tessellate_rounded_rect(surface: RenderSurface, cmd: &RectCommand, r: f32) -> Vec<ImmediateVertexBytes> {
    let pts = rounded_rect_outline(&cmd.rect, r);

    if cmd.filled {
        triangulate_polygon(surface, &pts, &cmd.color)
    } else {
        tessellate_polyline(
            surface,
            &PolylineCommand {
                points: pts,
                color: cmd.color.clone(),
                width: cmd.stroke_width as u32,
                closed: true,
                filled: false,
            },
        )
    }
}

/// Emit textured glyph-atlas quads for one text command. Pen advances use the
/// *same* `font::char_advance` the engine measured/wrapped with, so the drawn
/// run lands exactly where layout placed it. Glyphs outside the baked charset
/// still advance by their true width but draw nothing.
fn tessellate_text(surface: RenderSurface, cmd: &TextCommand, out: &mut Vec<TexturedVertex>) {
    const MAX_GLYPHS: usize = 16_384;
    const TAB_SPACES: u32 = 4;

    if cmd.size <= 0.0 {
        return;
    }

    let atlas = font::atlas();
    let size = cmd.size;
    let s = size / font::ATLAS_BASE_PX;
    let line_height = font::text_line_height(size);
    let tab_w = font::char_advance(' ', size) * TAB_SPACES as f32;

    let base_x = cmd.position.x;
    let mut pen_x = base_x;
    // The engine positions a line by its top-left; shift to the baseline to draw.
    let mut baseline_y = cmd.position.y + font::ascent(size);
    let color = color_to_array(&cmd.color);

    let mut glyph_count = 0usize;
    for ch in cmd.content.chars() {
        if glyph_count >= MAX_GLYPHS {
            break;
        }
        match ch {
            '\n' => {
                pen_x = base_x;
                baseline_y += line_height;
                continue;
            }
            '\t' => {
                pen_x += tab_w;
                continue;
            }
            _ => {}
        }

        if let Some(g) = atlas.glyph(ch) {
            if g.width > 0.0 && g.height > 0.0 {
                let dest = Rect {
                    x: pen_x + g.left * s,
                    y: baseline_y + g.top * s,
                    w: g.width * s,
                    h: g.height * s,
                };
                let uv = Rect {
                    x: g.u0,
                    y: g.v0,
                    w: g.u1 - g.u0,
                    h: g.v1 - g.v0,
                };
                out.extend(textured_quad(surface, &dest, &uv, color));
            }
        }
        pen_x += font::char_advance(ch, size);
        glyph_count += 1;
    }
}

fn polar(cmd: &CircleCommand, radius: f32, angle: f32) -> (f32, f32) {
    (
        cmd.center.x + radius.max(0.0) * angle.cos(),
        cmd.center.y + radius.max(0.0) * angle.sin(),
    )
}

fn to_clip(surface: RenderSurface, x: f32, y: f32) -> [f32; 2] {
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
struct RenderSurface {
    width: f32,
    height: f32,
}

impl RenderSurface {
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

const TEXTURED_SHADER: &str = r#"
struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

@vertex
fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
) -> VertexOut {
    var out: VertexOut;
    out.position = vec4(position, 0.0, 1.0);
    out.uv = uv;
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    return textureSample(tex, samp, in.uv) * in.color;
}
"#;
