use interstice_sdk::*;

interstice_module!(authorities: [Input, Gpu]);

// TABLES

#[table]
#[derive(Debug)]
pub struct PipelineTable {
    #[primary_key]
    pub id: u32,
    pub pipeline_id: u32,
}

#[table]
#[derive(Debug)]
pub struct VertexBufferTable {
    #[primary_key]
    pub id: u32,
    pub buffer_id: u32,
}

// REDUCERS

#[reducer(on = "init")]
pub fn init(ctx: ReducerContext) {
    let gpu = ctx.gpu();

    // Create shader
    let shader = gpu.create_shader_module(
        r#"
        @vertex
        fn vs_main(@builtin(vertex_index) vid: u32) -> @builtin(position) vec4<f32> {
            var pos = array<vec2<f32>, 3>(
                vec2(0.0, 0.5),
                vec2(-0.5, -0.5),
                vec2(0.5, -0.5),
            );
            let p = pos[vid];
            return vec4(p, 0.0, 1.0);
        }

        @fragment
        fn fs_main() -> @location(0) vec4<f32> {
            return vec4(1.0, 0.2, 0.3, 1.0);
        }
    "#
        .into(),
    );

    // Get surface format
    let surface_format = gpu.get_surface_format();

    // Create pipeline layout
    let pipeline_layout = gpu.create_pipeline_layout(CreatePipelineLayout {
        bind_group_layouts: vec![],
    });

    // Create render pipeline
    let pipeline = gpu.create_render_pipeline(CreateRenderPipeline {
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
    });

    // Store pipeline in table
    ctx.current.tables.pipelinetable().insert(PipelineTable {
        id: 0,
        pipeline_id: pipeline,
    });
}

#[reducer(on = "render")]
pub fn render(ctx: ReducerContext) {
    let gpu = ctx.gpu();

    // Get pipeline from table
    let pipeline = if let Some(p) = ctx.current.tables.pipelinetable().scan().get(0) {
        p.pipeline_id
    } else {
        return;
    };

    // Begin frame
    gpu.begin_frame();

    // Get current surface texture
    let surface_texture = gpu.get_current_surface_texture();
    let surface_format = gpu.get_surface_format();
    let surface_view = gpu.create_texture_view(CreateTextureView {
        texture: surface_texture,
        format: Some(surface_format),
        dimension: Some(TextureViewDimension::D2),
        base_mip_level: 0,
        mip_level_count: None,
        base_array_layer: 0,
        array_layer_count: None,
    });

    // Create command encoder
    let encoder = gpu.create_command_encoder();

    // Begin render pass
    let pass = gpu.begin_render_pass(BeginRenderPass {
        color_attachments: vec![RenderPassColorAttachment {
            view: surface_view,
            resolve_target: None,
            load: LoadOp::Clear,
            store: StoreOp::Store,
            clear_color: [0.1, 0.1, 0.1, 1.0],
        }],
        encoder,
        depth_stencil: None,
    });

    // Set pipeline and draw
    gpu.set_render_pipeline(pass, pipeline);
    gpu.draw(pass, 3, 1); // 3 vertices, 1 instance

    // End render pass
    gpu.end_render_pass(pass);

    // Submit and present
    gpu.submit(encoder);
    gpu.present();
}

#[reducer(on = "input")]
pub fn on_input(ctx: ReducerContext, event: InputEvent) {
    // Handle input events (unchanged)
    match event {
        InputEvent::Key {
            physical_key,
            state,
            ..
        } => {
            ctx.log(&format!("Key {:?} {:?}", physical_key, state));
        }
        _ => {}
    }
}
