mod api;
mod input;
mod layout;
mod render;
mod tables;
mod text;
mod types;

use interstice_sdk::*;
use tables::*;

interstice_module!(visibility: Public);

use crate::bindings::graphics::*;

pub const UI_LAYER: &str = "ui";
pub const UI_LAYER_Z: i32 = 100;

// ── Load ─────────────────────────────────────────────────────────────────────

#[reducer(on = "load")]
pub fn on_load<Caps>(ctx: ReducerContext<Caps>)
where
    Caps: CanInsert<InputFocus> + CanInsert<UiInputState>,
{
    let graphics = ctx.graphics();
    if let Err(err) = graphics
        .reducers
        .create_layer(UI_LAYER.to_string(), UI_LAYER_Z, false)
    {
        ctx.log(&format!("ui: failed to create layer: {err}"));
    }
    let _ = ctx.current.tables.inputfocus().insert(InputFocus {
        id: 0,
        focused_element: None,
    });
    let _ = ctx.current.tables.uiinputstate().insert(UiInputState {
        id: 0,
        last_input_generation: 0,
    });
}
