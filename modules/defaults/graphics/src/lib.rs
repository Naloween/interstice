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
