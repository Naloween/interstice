use interstice_sdk::*;

interstice_module!(visibility: Private, authorities: [Gpu]);

pub(crate) const RENDERER_CACHE_KEY: u32 = 0;
pub(crate) const MAX_DRAW_COMMANDS: usize = 16_384;
pub(crate) const MIN_SEGMENTS: u32 = 12;
pub(crate) const DEFAULT_SEGMENTS: u32 = 48;
pub(crate) const DEFAULT_CLEAR: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

mod draw;
mod helpers;
mod layers;
mod render;
mod resources;
mod tables;
mod types;

pub use draw::*;
pub use layers::*;
pub use render::render;
pub use resources::*;
pub use tables::*;
pub use types::*;

#[reducer(on = "load")]
pub fn load(ctx: ReducerContext) {
    if ctx
        .current
        .tables
        .layer()
        .get("default".to_string())
        .is_none()
    {
        let _ = ctx.current.tables.layer().insert(Layer {
            name: "default".to_string(),
            z: 0,
            clear: true,
            owner_node_id: ctx.caller_node_id.clone(),
        });
    }

    if ctx.current.tables.frametick().get(0).is_none() {
        let _ = ctx
            .current
            .tables
            .frametick()
            .insert(FrameTick { id: 0, frame: 0 });
    }

    // Drop GPU-backed resources from previous sessions; they are not valid after restart.
    if let Ok(rows) = ctx.current.tables.texturebinding().scan() {
        for row in rows {
            let _ = ctx.current.tables.texturebinding().delete(row.key);
        }
    }
    if let Ok(rows) = ctx.current.tables.meshbinding().scan() {
        for row in rows {
            let _ = ctx.current.tables.meshbinding().delete(row.key);
        }
    }

    // Clear cached GPU IDs so pipelines are recompiled with the current surface.
    if let Ok(mut rows) = ctx.current.tables.pipelinebinding().scan() {
        for mut row in rows {
            row.shader_module_id = None;
            row.pipeline_layout_id = None;
            row.pipeline_id = None;
            let _ = ctx.current.tables.pipelinebinding().update(row);
        }
    }
    if let Ok(mut rows) = ctx.current.tables.renderercache().scan() {
        for mut row in rows {
            row.surface_format = None;
            row.shader_module = None;
            row.pipeline_layout = None;
            row.pipeline_id = None;
            let _ = ctx.current.tables.renderercache().update(row);
        }
    }
}
