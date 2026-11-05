use interstice_sdk::*;

interstice_module!(authorities: [Gpu]);

// TABLES

#[table(ephemeral)]
#[derive(Debug)]
pub struct PipelineTable {
    #[primary_key]
    pub id: u32,
    pub pipeline_id: u32,
}

// REDUCERS

#[reducer(on = "load")]
pub fn load<Caps>(ctx: ReducerContext<Caps>)
where
    Caps: CanInsert<PipelineTable>,
{
    ctx.log("Loading graphics...");
    let gpu = ctx.gpu();

    // Create shader
    let shader = match gpu.create_shader_module(
        r#"
        // A raw-GPU triangle in the brand's light (brand/BRAND.md): `core` at
        // the top fading to `amber`, on `ground`. Values are sRGB, written to
        // the plain-unorm swapchain as they are (DESIGN §8 colour space).
        struct Out {
            @builtin(position) position: vec4<f32>,
            @location(0) color: vec3<f32>,
        };

        @vertex
        fn vs_main(@builtin(vertex_index) vid: u32) -> Out {
            var pos = array<vec2<f32>, 3>(
                vec2(0.0, 0.5),
                vec2(-0.5, -0.5),
                vec2(0.5, -0.5),
            );
            var col = array<vec3<f32>, 3>(
                vec3(0.965, 0.914, 0.824),
                vec3(0.773, 0.608, 0.376),
                vec3(0.773, 0.608, 0.376),
            );
            var out: Out;
            out.position = vec4(pos[vid], 0.0, 1.0);
            out.color = col[vid];
            return out;
        }

        @fragment
        fn fs_main(in: Out) -> @location(0) vec4<f32> {
            return vec4(in.color, 1.0);
        }
    "#
        .into(),
    ) {
        Ok(id) => id,
        Err(err) => {
            ctx.log(&format!("Failed to create shader module: {}", err));
            return;
        }
    };

    // Get surface format
    let surface_format = match gpu.get_surface_format() {
        Ok(format) => format,
        Err(err) => {
            ctx.log(&format!("Failed to get surface format: {}", err));
            return;
        }
    };

    // Create pipeline layout
    let pipeline_layout = match gpu.create_pipeline_layout(CreatePipelineLayout {
        bind_group_layouts: vec![],
    }) {
        Ok(id) => id,
        Err(err) => {
            ctx.log(&format!("Failed to create pipeline layout: {}", err));
            return;
        }
    };

    // Create render pipeline
    let pipeline = match gpu.create_render_pipeline(CreateRenderPipeline {
        label: Some("main".into()),
        layout: pipeline_layout,
        vertex: VertexState {
            module: shader,
            entry_point: "vs_main".into(),
            buffers: vec![],
        },
        fragment: Some(FragmentState {
            module: shader,
            entry_point: "fs_main".into(),
            targets: vec![ColorTargetState {
                format: surface_format,
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
    }) {
        Ok(id) => id,
        Err(err) => {
            ctx.log(&format!("Failed to create render pipeline: {}", err));
            return;
        }
    };

    // Store pipeline in table

    if let Err(err) = ctx.current.tables.pipelinetable().insert(PipelineTable {
        id: 0,
        pipeline_id: pipeline,
    }) {
        ctx.log(&format!("Failed to insert pipeline table: {}", err));
    }
}

#[reducer(on = "render")]
pub fn render<Caps>(ctx: ReducerContext<Caps>) -> Result<(), String>
where
    Caps: CanRead<PipelineTable>,
{
    let gpu = ctx.gpu();

    // Get pipeline from table
    let pipeline_rows = ctx.current.tables.pipelinetable().scan()?;
    let pipeline = match pipeline_rows.get(0) {
        Some(p) => p.pipeline_id,
        None => return Ok(()),
    };

    // Begin frame
    if let Err(err) = gpu.begin_frame() {
        ctx.log(&format!("Failed to begin frame: {}", err));
        return Ok(());
    }

    // Get current surface texture
    let surface_texture = match gpu.get_current_surface_texture() {
        Ok(id) => id,
        Err(err) => {
            ctx.log(&format!("Failed to get surface texture: {}", err));
            return Ok(());
        }
    };
    let surface_format = match gpu.get_surface_format() {
        Ok(format) => format,
        Err(err) => {
            ctx.log(&format!("Failed to get surface format: {}", err));
            return Ok(());
        }
    };
    let surface_view = match gpu.create_texture_view(CreateTextureView {
        texture: surface_texture,
        format: Some(surface_format),
        dimension: Some(TextureViewDimension::D2),
        base_mip_level: 0,
        mip_level_count: None,
        base_array_layer: 0,
        array_layer_count: None,
    }) {
        Ok(id) => id,
        Err(err) => {
            ctx.log(&format!("Failed to create surface view: {}", err));
            return Ok(());
        }
    };

    // Create command encoder
    let encoder = match gpu.create_command_encoder() {
        Ok(id) => id,
        Err(err) => {
            ctx.log(&format!("Failed to create command encoder: {}", err));
            return Ok(());
        }
    };

    // Begin render pass
    let pass = match gpu.begin_render_pass(BeginRenderPass {
        color_attachments: vec![RenderPassColorAttachment {
            view: surface_view,
            resolve_target: None,
            load: LoadOp::Clear,
            store: StoreOp::Store,
            // `ground` (#151618).
            clear_color: [0.082, 0.086, 0.094, 1.0],
        }],
        encoder,
        depth_stencil: None,
    }) {
        Ok(id) => id,
        Err(err) => {
            ctx.log(&format!("Failed to begin render pass: {}", err));
            return Ok(());
        }
    };

    // Set pipeline and draw
    if let Err(err) = gpu.set_render_pipeline(pass, pipeline) {
        ctx.log(&format!("Failed to set render pipeline: {}", err));
        return Ok(());
    }
    if let Err(err) = gpu.draw(pass, 3, 1) {
        ctx.log(&format!("Failed to draw: {}", err));
        return Ok(());
    }

    // End render pass
    if let Err(err) = gpu.end_render_pass(pass) {
        ctx.log(&format!("Failed to end render pass: {}", err));
        return Ok(());
    }

    // Submit and present
    if let Err(err) = gpu.submit(encoder) {
        ctx.log(&format!("Failed to submit encoder: {}", err));
        return Ok(());
    }
    if let Err(err) = gpu.present() {
        ctx.log(&format!("Failed to present frame: {}", err));
    }
    Ok(())
}

// #[reducer(on = "input")]
// pub fn on_input(ctx: ReducerContext, event: InputEvent) {
//     // Handle input events (unchanged)
//     match event {
//         InputEvent::Key {
//             physical_key,
//             state,
//             ..
//         } => {
//             ctx.log(&format!("Key {:?} {:?}", physical_key, state));
//         }
//         _ => {}
//     }
// }
