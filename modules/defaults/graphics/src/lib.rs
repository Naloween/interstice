use interstice_sdk::*;

interstice_module!(visibility: Private, authorities: [Gpu]);

pub(crate) const RENDERER_CACHE_KEY: u32 = 0;
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
    let _ = ctx.current.tables.layer().insert(Layer {
        name: "default".to_string(),
        z: 0,
        clear: true,
        owner_module_name: "graphics".to_string(),
    });

    ctx.current
        .tables
        .frametick()
        .insert(FrameTick { id: 0, frame: 0 })
        .expect("Couldn't insert frame count");
}
