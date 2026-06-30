//! Builders for the browser's chrome and content `UiElement`s (toolbar buttons,
//! the URL bar, text/space blocks) plus the small per-frame helpers that keep the
//! toolbar in sync with navigation state.

use interstice_sdk::*;

use crate::tables::*;
use crate::ui;
use crate::ui::*;

/// A block-level text element with the box model resolved by the CSS engine:
/// per-side `margin`/`padding` (each `(top, right, bottom, left)` px), a `width`
/// (px / % / fill), and an optional border. `None`/zero box values collapse to
/// today's flush, full-width behaviour.
#[allow(clippy::too_many_arguments)]
pub(crate) fn text_el(
    id: String,
    parent: String,
    order: u32,
    text: String,
    size: f32,
    color: (f32, f32, f32, f32),
    bold: bool,
    italic: bool,
    align: f32,
    background: Option<(f32, f32, f32, f32)>,
    margin: (f32, f32, f32, f32),
    padding: (f32, f32, f32, f32),
    width: Size,
    height: Size,
    line_height: f32,
    border_width: f32,
    border_color: (f32, f32, f32, f32),
    spans: Vec<ui::TextSpan>,
) -> UiElement {
    let zero = (0.0, 0.0, 0.0, 0.0);
    UiElement {
        id,
        parent: Some(parent),
        order,
        width,
        height,
        layout_direction: LayoutDirection::Row,
        gap: 0.0,
        padding: 0.0,
        margin: 0.0,
        padding_sides: if padding != zero { Some(padding) } else { None },
        margin_sides: if margin != zero { Some(margin) } else { None },
        background_color: background.unwrap_or(TRANSPARENT),
        corner_radius: 0.0,
        border_width,
        border_color,
        text: Some(text),
        text_size: size,
        text_color: color,
        text_bold: bold,
        text_italic: italic,
        text_wrap: TextWrap::Words,
        text_align: align,
        line_height,
        spans,
        image: None,
        is_input: false,
        cursor_pos: 0,
        scrollable_x: false,
        scrollable_y: false,
        scroll_x: 0.0,
        scroll_y: 0.0,
        visible: true,
        ..Default::default()
    }
}

pub(crate) fn space_el(id: String, parent: String, order: u32, height: f32) -> UiElement {
    UiElement {
        id,
        parent: Some(parent),
        order,
        width: Size::Grow,
        height: Size::Fixed(height),
        layout_direction: LayoutDirection::Row,
        gap: 0.0,
        padding: 0.0,
        margin: 0.0,
        background_color: TRANSPARENT,
        corner_radius: 0.0,
        border_width: 0.0,
        border_color: TRANSPARENT,
        text: None,
        text_size: 0.0,
        text_color: TRANSPARENT,
        text_wrap: TextWrap::None,
        image: None,
        is_input: false,
        cursor_pos: 0,
        scrollable_x: false,
        scrollable_y: false,
        scroll_x: 0.0,
        scroll_y: 0.0,
        visible: true,
        ..Default::default()
    }
}

/// A toolbar button (back / forward). Lives in the toolbar row; `label` is a short
/// glyph. Text colour is set per-frame to reflect whether it's actionable.
pub(crate) fn nav_button_el(id: String, order: u32, label: &str) -> UiElement {
    UiElement {
        id,
        parent: Some(TOOLBAR_ID.into()),
        order,
        width: Size::Fixed(34.0),
        height: Size::Grow,
        layout_direction: LayoutDirection::Row,
        gap: 0.0,
        padding: 10.0,
        margin: 0.0,
        background_color: BTN_BG,
        corner_radius: 6.0,
        border_width: 1.0,
        border_color: BTN_BORDER,
        text: Some(label.to_string()),
        text_size: 16.0,
        text_color: BTN_TEXT_DIM,
        text_wrap: TextWrap::None,
        image: None,
        is_input: false,
        cursor_pos: 0,
        scrollable_x: false,
        scrollable_y: false,
        scroll_x: 0.0,
        scroll_y: 0.0,
        visible: true,
        ..Default::default()
    }
}

/// Brighten or dim a toolbar button's label to signal whether it's actionable.
pub(crate) fn update_button_state<Caps>(ctx: &ReducerContext<Caps>, id: &str, enabled: bool)
where
    Caps: CanRead<ui::UiElement> + CanUpdate<ui::UiElement>,
{
    if let Some(mut btn) = ctx.current.tables.uielement().get(id.to_string()) {
        let want = if enabled { BTN_TEXT } else { BTN_TEXT_DIM };
        if btn.text_color != want {
            btn.text_color = want;
            let _ = ctx.current.tables.uielement().update(btn);
        }
    }
}

/// Reflect `url` in the address bar (text + caret at end).
pub(crate) fn set_urlbar<Caps>(ctx: &ReducerContext<Caps>, url: &str)
where
    Caps: CanRead<ui::UiElement> + CanUpdate<ui::UiElement>,
{
    if let Some(mut bar) = ctx.current.tables.uielement().get(URLBAR_ID.to_string()) {
        bar.text = Some(url.to_string());
        bar.cursor_pos = url.chars().count() as u32;
        let _ = ctx.current.tables.uielement().update(bar);
    }
}

/// A placeholder box shown for an `<img>` until its fetch decodes (or fails). Once
/// the texture arrives, [`crate::images::place_image`] swaps the element to the
/// real image at its display size.
pub(crate) fn image_placeholder_el(id: String, parent: String, order: u32) -> UiElement {
    UiElement {
        id,
        parent: Some(parent),
        order,
        width: Size::Fixed(220.0),
        height: Size::Fixed(140.0),
        layout_direction: LayoutDirection::Row,
        gap: 0.0,
        padding: 0.0,
        margin: 6.0,
        background_color: IMG_PLACEHOLDER_BG,
        corner_radius: 4.0,
        border_width: 0.0,
        border_color: TRANSPARENT,
        text: None,
        text_size: 0.0,
        text_color: TRANSPARENT,
        text_wrap: TextWrap::None,
        image: None,
        is_input: false,
        cursor_pos: 0,
        scrollable_x: false,
        scrollable_y: false,
        scroll_x: 0.0,
        scroll_y: 0.0,
        visible: true,
        ..Default::default()
    }
}
