use interstice_sdk::*;

#[interstice_type]
#[derive(Debug, PartialEq)]
pub enum LayoutDirection {
    Row,
    Column,
}

#[interstice_type]
#[derive(Debug, PartialEq)]
pub enum Size {
    Fixed(f32),
    Grow,
    Fit,
}

#[interstice_type]
#[derive(Debug, PartialEq)]
pub enum TextWrap {
    None,
    Words,
    Newlines,
}

/// The canonical UI element used by the layout and draw engine. Each consuming
/// module declares its own `#[table]` row with the identical field set (emitted
/// by [`crate::ui_subsystem`]) and converts into this type before laying out.
#[derive(Clone)]
pub struct UiElement {
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
    /// Texture local_id of an image to draw into this element's content box.
    /// `None` for ordinary boxes; set for `<img>`-style elements.
    pub image: Option<String>,
    pub is_input: bool,
    pub cursor_pos: u32,
    pub scrollable_x: bool,
    pub scrollable_y: bool,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub visible: bool,
}
