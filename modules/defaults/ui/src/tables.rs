use crate::types::*;
use interstice_sdk::*;

#[table(public)]
pub struct UiElement {
    #[primary_key]
    pub id: String,
    pub parent: Option<String>,
    pub order: u32,
    pub width: Size,
    pub height: Size,
    pub layout_direction: LayoutDirection,
    pub gap: f32,
    pub padding: f32,
    pub margin: f32,
    pub background_color: (f32, f32, f32, f32),
    pub corner_radius: f32,
    pub border_width: f32,
    pub border_color: (f32, f32, f32, f32),
    pub text: Option<String>,
    pub text_size: f32,
    pub text_color: (f32, f32, f32, f32),
    pub text_wrap: TextWrap,
    // Text input
    pub is_input: bool,
    pub cursor_pos: u32,
    // Scroll
    pub scrollable_x: bool,
    pub scrollable_y: bool,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub visible: bool,
}

/// Which element currently has keyboard focus.
#[table(ephemeral)]
pub struct InputFocus {
    #[primary_key]
    pub id: u32,
    pub focused_element: Option<String>,
}

/// Persistent bookkeeping to detect new input events each frame.
#[table]
pub struct UiInputState {
    #[primary_key]
    pub id: u32,
    pub last_input_generation: u64,
}
