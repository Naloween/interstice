use crate::bindings::{graphics::*, input::*, ui::*};
use interstice_sdk::*;

interstice_module!(visibility: Public);

const BUTTON_ID: &str = "btn_primary";
const CARD_BG: (f32, f32, f32, f32) = (0.18, 0.18, 0.22, 1.0);
const BUTTON_BG: (f32, f32, f32, f32) = (0.27, 0.47, 0.87, 1.0);
const BUTTON_HOVER_BG: (f32, f32, f32, f32) = (0.35, 0.55, 0.95, 1.0);
const TEXT_COLOR: (f32, f32, f32, f32) = (0.92, 0.92, 0.95, 1.0);
const MUTED_COLOR: (f32, f32, f32, f32) = (0.60, 0.60, 0.65, 1.0);

#[reducer(on = "load")]
pub fn on_load(ctx: ReducerContext) {
    let ui = ctx.ui();

    // Root: full-screen dark background
    let _ = ui.reducers.create_element(UiElement {
        id: "root".into(),
        parent: None,
        order: 0,
        width: Size::Grow,
        height: Size::Grow,
        layout_direction: LayoutDirection::Column,
        gap: 0.0,
        padding: 0.0,
        margin: 0.0,
        background_color: (0.10, 0.10, 0.13, 1.0),
        corner_radius: 0.0,
        border_width: 0.0,
        border_color: (0.0, 0.0, 0.0, 0.0),
        text: None,
        text_size: 0.0,
        text_color: (0.0, 0.0, 0.0, 0.0),
        text_wrap: TextWrap::Words,
        is_input: false,
        cursor_pos: 0,
        scrollable_x: false,
        scrollable_y: false,
        scroll_x: 0.0,
        scroll_y: 0.0,
        visible: true,
    });

    // Main panel — fills the window with a margin from the border
    let _ = ui.reducers.create_element(UiElement {
        id: "card".into(),
        parent: Some("root".into()),
        order: 0,
        width: Size::Grow,
        height: Size::Grow,
        layout_direction: LayoutDirection::Column,
        gap: 12.0,
        padding: 28.0,
        margin: 32.0,
        background_color: CARD_BG,
        corner_radius: 12.0,
        border_width: 1.0,
        border_color: (0.30, 0.30, 0.36, 1.0),
        text: None,
        text_size: 0.0,
        text_color: (0.0, 0.0, 0.0, 0.0),
        text_wrap: TextWrap::Words,
        is_input: false,
        cursor_pos: 0,
        scrollable_x: false,
        scrollable_y: false,
        scroll_x: 0.0,
        scroll_y: 0.0,
        visible: true,
    });

    // Title
    let _ = ui.reducers.create_element(UiElement {
        id: "title".into(),
        parent: Some("card".into()),
        order: 0,
        width: Size::Grow,
        height: Size::Fit,
        layout_direction: LayoutDirection::Row,
        gap: 0.0,
        padding: 0.0,
        margin: 0.0,
        background_color: (0.0, 0.0, 0.0, 0.0),
        corner_radius: 0.0,
        border_width: 0.0,
        border_color: (0.0, 0.0, 0.0, 0.0),
        text: Some("Interstice UI".into()),
        text_size: 24.0,
        text_color: TEXT_COLOR,
        text_wrap: TextWrap::Words,
        is_input: false,
        cursor_pos: 0,
        scrollable_x: false,
        scrollable_y: false,
        scroll_x: 0.0,
        scroll_y: 0.0,
        visible: true,
    });

    // Subtitle
    let _ = ui.reducers.create_element(UiElement {
        id: "subtitle".into(),
        parent: Some("card".into()),
        order: 1,
        width: Size::Grow,
        height: Size::Fit,
        layout_direction: LayoutDirection::Row,
        gap: 0.0,
        padding: 0.0,
        margin: 0.0,
        background_color: (0.0, 0.0, 0.0, 0.0),
        corner_radius: 0.0,
        border_width: 0.0,
        border_color: (0.0, 0.0, 0.0, 0.0),
        text: Some("Default module — layout example".into()),
        text_size: 14.0,
        text_color: MUTED_COLOR,
        text_wrap: TextWrap::Words,
        is_input: false,
        cursor_pos: 0,
        scrollable_x: false,
        scrollable_y: false,
        scroll_x: 0.0,
        scroll_y: 0.0,
        visible: true,
    });

    // Separator (thin horizontal bar)
    let _ = ui.reducers.create_element(UiElement {
        id: "sep".into(),
        parent: Some("card".into()),
        order: 2,
        width: Size::Grow,
        height: Size::Fixed(1.0),
        layout_direction: LayoutDirection::Row,
        gap: 0.0,
        padding: 0.0,
        margin: 0.0,
        background_color: (0.30, 0.30, 0.36, 1.0),
        corner_radius: 0.0,
        border_width: 0.0,
        border_color: (0.0, 0.0, 0.0, 0.0),
        text: None,
        text_size: 0.0,
        text_color: (0.0, 0.0, 0.0, 0.0),
        text_wrap: TextWrap::Words,
        is_input: false,
        cursor_pos: 0,
        scrollable_x: false,
        scrollable_y: false,
        scroll_x: 0.0,
        scroll_y: 0.0,
        visible: true,
    });

    // Info rows
    for (i, label) in ["Module: ui-example", "Node: local", "Status: running"]
        .iter()
        .enumerate()
    {
        let _ = ui.reducers.create_element(UiElement {
            id: format!("row_{i}"),
            parent: Some("card".into()),
            order: (3 + i) as u32,
            width: Size::Grow,
            height: Size::Fit,
            layout_direction: LayoutDirection::Row,
            gap: 0.0,
            padding: 6.0,
            margin: 0.0,
            background_color: (0.14, 0.14, 0.18, 1.0),
            corner_radius: 6.0,
            border_width: 0.0,
            border_color: (0.0, 0.0, 0.0, 0.0),
            text: Some(label.to_string()),
            text_size: 13.0,
            text_color: TEXT_COLOR,
            text_wrap: TextWrap::Words,
            is_input: false,
            cursor_pos: 0,
            scrollable_x: false,
            scrollable_y: false,
            scroll_x: 0.0,
            scroll_y: 0.0,
            visible: true,
        });
    }

    // Primary button
    let _ = ui.reducers.create_element(UiElement {
        id: BUTTON_ID.into(),
        parent: Some("card".into()),
        order: 6,
        width: Size::Grow,
        height: Size::Fixed(40.0),
        layout_direction: LayoutDirection::Row,
        gap: 0.0,
        padding: 8.0,
        margin: 4.0,
        background_color: BUTTON_BG,
        corner_radius: 8.0,
        border_width: 0.0,
        border_color: (0.0, 0.0, 0.0, 0.0),
        text: Some("Click me".into()),
        text_size: 14.0,
        text_color: (1.0, 1.0, 1.0, 1.0),
        text_wrap: TextWrap::Words,
        is_input: false,
        cursor_pos: 0,
        scrollable_x: false,
        scrollable_y: false,
        scroll_x: 0.0,
        scroll_y: 0.0,
        visible: true,
    });
}

#[reducer(on = "graphics.frametick.update")]
pub fn on_frame<Caps>(ctx: ReducerContext<Caps>, _prev: FrameTick, _tick: FrameTick)
where
    Caps: CanRead<MouseState>,
{
    let (mx, my) = ctx
        .input()
        .tables
        .mousestate()
        .get(0)
        .map(|m| m.position)
        .unwrap_or((0.0, 0.0));

    // Resolve button bounds from UI tables — not trivially available without a
    // query, so we use a fixed estimate based on card position for hover check.
    // A future ui.query_bounds(id) API would replace this heuristic.
    let hovered = is_over_button(mx, my);

    let ui = ctx.ui();
    let bg = if hovered { BUTTON_HOVER_BG } else { BUTTON_BG };
    let _ = ui.reducers.update_element(UiElement {
        id: BUTTON_ID.into(),
        parent: Some("card".into()),
        order: 6,
        width: Size::Grow,
        height: Size::Fixed(40.0),
        layout_direction: LayoutDirection::Row,
        gap: 0.0,
        padding: 8.0,
        margin: 4.0,
        background_color: bg,
        corner_radius: 8.0,
        border_width: 0.0,
        border_color: (0.0, 0.0, 0.0, 0.0),
        text: Some("Click me".into()),
        text_size: 14.0,
        text_color: (1.0, 1.0, 1.0, 1.0),
        text_wrap: TextWrap::Words,
        is_input: false,
        cursor_pos: 0,
        scrollable_x: false,
        scrollable_y: false,
        scroll_x: 0.0,
        scroll_y: 0.0,
        visible: true,
    });
}

// Placeholder: approximate button region.
// TODO: replace with a bounds query once the UI module exposes one.
fn is_over_button(mx: f32, my: f32) -> bool {
    // Card is Fixed(420) wide, centered — we don't know center without surface size here.
    // Input module tracks cursor delta not absolute position yet, so this always returns false.
    // Will be refined when absolute cursor position is available.
    let _ = (mx, my);
    false
}
