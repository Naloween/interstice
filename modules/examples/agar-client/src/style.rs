//! Element builders in the brand (brand/BRAND.md §7), from `interstice_ui::theme`.

use crate::ui::{AlignItems, JustifyContent, LayoutDirection, Size, TextWrap, UiElement};
use interstice_ui::theme::{self as th, Rgba};

pub const CLEAR: Rgba = (0.0, 0.0, 0.0, 0.0);

/// A box: `dir` children, no fill.
pub fn boxed(id: &str, parent: Option<&str>, order: u32, dir: LayoutDirection, w: Size, h: Size) -> UiElement {
    UiElement {
        id: id.into(),
        parent: parent.map(Into::into),
        order,
        width: w,
        height: h,
        layout_direction: dir,
        ..Default::default()
    }
}

/// A full-surface root that centres its single child.
pub fn centred_root(id: &str, order: u32, bg: Rgba) -> UiElement {
    UiElement {
        background_color: bg,
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..boxed(id, None, order, LayoutDirection::Column, Size::Grow, Size::Grow)
    }
}

/// A panel: charcoal, hairline border, window radius.
pub fn panel(id: &str, parent: &str, order: u32, w: Size, padding: f32, gap: f32) -> UiElement {
    UiElement {
        padding,
        gap,
        background_color: th::CHARCOAL,
        corner_radius: th::RADIUS_TILE,
        border_width: 1.0,
        border_color: th::LINE,
        ..boxed(id, Some(parent), order, LayoutDirection::Column, w, Size::Fit)
    }
}

pub fn text(id: &str, parent: &str, order: u32, body: &str, size: f32, color: Rgba) -> UiElement {
    UiElement {
        text: Some(body.into()),
        text_size: size,
        text_color: color,
        text_wrap: TextWrap::Words,
        ..boxed(id, Some(parent), order, LayoutDirection::Row, Size::Grow, Size::Fit)
    }
}

/// An uppercase, tracked caption label (BRAND.md §4).
pub fn label(id: &str, parent: &str, order: u32, body: &str, color: Rgba) -> UiElement {
    UiElement { text_wrap: TextWrap::None, ..text(id, parent, order, &th::label(body), th::TEXT_CAPTION, color) }
}

/// Mono text, for numbers that change.
pub fn mono(id: &str, parent: &str, order: u32, body: &str, size: f32, color: Rgba) -> UiElement {
    UiElement { text_mono: true, text_wrap: TextWrap::None, ..text(id, parent, order, body, size, color) }
}

/// The view's one primary button: amber, ink label.
pub fn primary_button(id: &str, parent: &str, order: u32, body: &str, hover: bool) -> UiElement {
    UiElement {
        height: Size::Fixed(40.0),
        padding: 10.0,
        background_color: if hover { th::AMBER_HI } else { th::AMBER },
        corner_radius: th::RADIUS_CONTROL,
        text_color: th::INK,
        text_bold: true,
        text_align: 0.5,
        text_wrap: TextWrap::None,
        ..text(id, parent, order, body, th::TEXT_READING, th::INK)
    }
}

/// A single-line text input.
pub fn input(id: &str, parent: &str, order: u32, value: &str) -> UiElement {
    UiElement {
        height: Size::Fixed(40.0),
        padding: 11.0,
        background_color: th::INK,
        corner_radius: th::RADIUS_CONTROL,
        border_width: 1.0,
        border_color: th::INPUT_BORDER,
        is_input: true,
        cursor_pos: value.chars().count() as u32,
        text_wrap: TextWrap::None,
        ..text(id, parent, order, value, th::TEXT_READING, th::IVORY)
    }
}

/// A 1 px hairline rule.
pub fn rule(id: &str, parent: &str, order: u32) -> UiElement {
    UiElement {
        background_color: th::LINE,
        ..boxed(id, Some(parent), order, LayoutDirection::Row, Size::Grow, Size::Fixed(1.0))
    }
}

/// Vertical space.
pub fn gap(id: &str, parent: &str, order: u32, h: f32) -> UiElement {
    boxed(id, Some(parent), order, LayoutDirection::Row, Size::Grow, Size::Fixed(h))
}
