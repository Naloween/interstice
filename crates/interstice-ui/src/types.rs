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
    /// A fraction (0.0–1.0) of the available size along this axis (CSS `%`).
    Percent(f32),
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

/// Main-axis distribution of children within a Row/Column container (CSS
/// `justify-content`). Only takes effect when there is free main-axis space —
/// i.e. no `Size::Grow` child has soaked up the slack. `Start` ⇒ today's
/// packed-from-the-start behaviour.
#[interstice_type]
#[derive(Debug, PartialEq)]
pub enum JustifyContent {
    Start,
    Center,
    End,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

/// Cross-axis alignment of each child within the container's cross extent (CSS
/// `align-items`). `Stretch` ⇒ today's behaviour (a `Size::Grow` cross size
/// fills; other sizes sit at the start). `Center`/`End` offset the child by its
/// resolved cross size.
#[interstice_type]
#[derive(Debug, PartialEq)]
pub enum AlignItems {
    Start,
    Center,
    End,
    Stretch,
}

/// CSS `position`. `Static` ⇒ normal flow (today's behaviour). `Relative` lays
/// the box out in flow, then shifts it by its `pos_*` offsets without disturbing
/// siblings, and establishes a containing block for absolute descendants.
/// `Absolute` removes the box from flow and positions it (via `pos_*`) against
/// the content box of its nearest positioned ancestor (or the surface).
#[interstice_type]
#[derive(Debug, PartialEq)]
pub enum Position {
    Static,
    Relative,
    Absolute,
}

/// An inline run of styled text within a single rich-text [`UiElement`]. `start`
/// and `end` are **char** offsets into the element's `text` (half-open
/// `[start, end)`). A span overrides the element's base `text_color` for that
/// range and, when `href` is set, marks the range as a clickable link (drawn
/// underlined; resolved by [`crate::link_at`]). An empty `spans` list ⇒ the
/// element draws as plain single-colour text (the pre-span behaviour).
#[interstice_type]
#[derive(Debug, PartialEq)]
pub struct TextSpan {
    pub start: u32,
    pub end: u32,
    pub color: (f32, f32, f32, f32),
    pub href: Option<String>,
    /// Resolved weight/slant for this run (already folded with the element's
    /// base style by the caller), driving which DejaVu face draws it.
    pub bold: bool,
    pub italic: bool,
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
    /// Main-axis distribution of children (CSS `justify-content`).
    pub justify_content: JustifyContent,
    /// Cross-axis alignment of children (CSS `align-items`).
    pub align_items: AlignItems,
    /// CSS `position`. `Static` ⇒ normal flow.
    pub position: Position,
    /// `position` offsets `(left, top, right, bottom)`; `None` ⇒ `auto`. For
    /// `Relative` an unset side is 0; for `Absolute` the box anchors to whichever
    /// of each axis's pair is set (left/right, top/bottom).
    pub pos_left: Option<f32>,
    pub pos_top: Option<f32>,
    pub pos_right: Option<f32>,
    pub pos_bottom: Option<f32>,
    pub gap: f32,
    pub padding: f32,
    pub margin: f32,
    /// Optional per-side padding override `(top, right, bottom, left)`. `None` ⇒
    /// the uniform `padding` applies to all four sides. Set by callers that need
    /// CSS box-model per-side insets without disturbing the scalar shorthand.
    pub padding_sides: Option<(f32, f32, f32, f32)>,
    /// Optional per-side margin override `(top, right, bottom, left)`. `None` ⇒
    /// the uniform `margin` applies to all four sides.
    pub margin_sides: Option<(f32, f32, f32, f32)>,
    pub background_color: (f32, f32, f32, f32),
    pub corner_radius: f32,
    pub border_width: f32,
    pub border_color: (f32, f32, f32, f32),
    pub text: Option<String>,
    pub text_size: f32,
    pub text_color: (f32, f32, f32, f32),
    pub text_wrap: TextWrap,
    /// Base weight/slant for the whole element (the block's resolved style).
    /// Inline `spans` may override per-range; uncovered chars use these.
    pub text_bold: bool,
    pub text_italic: bool,
    /// Inline style runs over `text` (link/colour). Empty ⇒ plain text.
    pub spans: Vec<TextSpan>,
    /// Horizontal alignment of text lines within the content box: 0.0 = left,
    /// 0.5 = centre, 1.0 = right (CSS `text-align`).
    pub text_align: f32,
    /// Explicit line height in px (CSS `line-height`). `<= 0.0` ⇒ use the font's
    /// natural line height for the element's size/style. Drives both inter-line
    /// spacing when drawing and the measured text height during layout.
    pub line_height: f32,
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

impl UiElement {
    /// The element's base text style (weight/slant) for the plain-text path and
    /// as the fallback for chars no span covers.
    pub fn base_font_style(&self) -> crate::text::FontStyle {
        crate::text::FontStyle {
            bold: self.text_bold,
            italic: self.text_italic,
        }
    }
    /// Effective line height in px: the explicit `line_height` override when
    /// positive, else the font's natural line height for this element's size and
    /// base style. Used everywhere line spacing is needed (layout + draw) so the
    /// two stay in lockstep.
    pub fn line_height_px(&self) -> f32 {
        if self.line_height > 0.0 {
            self.line_height
        } else {
            crate::text::text_line_height(self.text_size, self.base_font_style())
        }
    }
    /// Effective per-side padding `(top, right, bottom, left)` — the override if
    /// set, else the uniform scalar on every side.
    pub fn pad(&self) -> (f32, f32, f32, f32) {
        self.padding_sides
            .unwrap_or((self.padding, self.padding, self.padding, self.padding))
    }
    /// Effective per-side margin `(top, right, bottom, left)`.
    pub fn mrg(&self) -> (f32, f32, f32, f32) {
        self.margin_sides
            .unwrap_or((self.margin, self.margin, self.margin, self.margin))
    }
    /// Total horizontal / vertical padding and the left / top inset.
    pub fn pad_x(&self) -> f32 {
        let (_, r, _, l) = self.pad();
        l + r
    }
    pub fn pad_y(&self) -> f32 {
        let (t, _, b, _) = self.pad();
        t + b
    }
    pub fn pad_l(&self) -> f32 {
        self.pad().3
    }
    pub fn pad_t(&self) -> f32 {
        self.pad().0
    }
    /// Total horizontal / vertical margin and the left / top inset.
    pub fn mrg_x(&self) -> f32 {
        let (_, r, _, l) = self.mrg();
        l + r
    }
    pub fn mrg_y(&self) -> f32 {
        let (t, _, b, _) = self.mrg();
        t + b
    }
    pub fn mrg_l(&self) -> f32 {
        self.mrg().3
    }
    pub fn mrg_t(&self) -> f32 {
        self.mrg().0
    }
}
