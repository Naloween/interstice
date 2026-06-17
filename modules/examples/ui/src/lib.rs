use crate::bindings::{graphics::*, input::*};
use interstice_sdk::*;

interstice_module!(visibility: Public);

// Paste a module-local UI subsystem (tables, helpers, render, key reducer) wired
// to this module's own graphics/input bindings. Everything below draws into our
// OWN layers, so the desktop compositor can route us to our own surface.
interstice_ui::ui_subsystem!();

use crate::ui::{LayoutDirection, Size, TextWrap, UiElement};

const BUTTON_ID: &str = "btn_primary";
const CARD_BG: (f32, f32, f32, f32) = (0.18, 0.18, 0.22, 1.0);
const BUTTON_BG: (f32, f32, f32, f32) = (0.27, 0.47, 0.87, 1.0);
const BUTTON_HOVER_BG: (f32, f32, f32, f32) = (0.35, 0.55, 0.95, 1.0);
const TEXT_COLOR: (f32, f32, f32, f32) = (0.92, 0.92, 0.95, 1.0);
const MUTED_COLOR: (f32, f32, f32, f32) = (0.60, 0.60, 0.65, 1.0);

#[reducer(on = "load")]
pub fn on_load<Caps>(ctx: ReducerContext<Caps>)
where
    Caps: CanInsert<ui::InputFocus> + CanInsert<ui::UiElement> + CanUpdate<ui::UiElement>,
{
    ui::install(&ctx);

    // Root: full-screen dark background
    ui::create_element(
        &ctx,
        UiElement {
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
            image: None,
            is_input: false,
            cursor_pos: 0,
            scrollable_x: false,
            scrollable_y: false,
            scroll_x: 0.0,
            scroll_y: 0.0,
            visible: true,
            ..Default::default()
        },
    );

    // Main panel — fills the window with a margin from the border
    ui::create_element(
        &ctx,
        UiElement {
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
            image: None,
            is_input: false,
            cursor_pos: 0,
            scrollable_x: false,
            scrollable_y: false,
            scroll_x: 0.0,
            scroll_y: 0.0,
            visible: true,
            ..Default::default()
        },
    );

    // Title
    ui::create_element(
        &ctx,
        UiElement {
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
            image: None,
            is_input: false,
            cursor_pos: 0,
            scrollable_x: false,
            scrollable_y: false,
            scroll_x: 0.0,
            scroll_y: 0.0,
            visible: true,
            ..Default::default()
        },
    );

    // Subtitle
    ui::create_element(
        &ctx,
        UiElement {
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
            image: None,
            is_input: false,
            cursor_pos: 0,
            scrollable_x: false,
            scrollable_y: false,
            scroll_x: 0.0,
            scroll_y: 0.0,
            visible: true,
            ..Default::default()
        },
    );

    // Separator (thin horizontal bar)
    ui::create_element(
        &ctx,
        UiElement {
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
            image: None,
            is_input: false,
            cursor_pos: 0,
            scrollable_x: false,
            scrollable_y: false,
            scroll_x: 0.0,
            scroll_y: 0.0,
            visible: true,
            ..Default::default()
        },
    );

    // Info rows
    for (i, label) in ["Module: ui-example", "Node: local", "Status: running"]
        .iter()
        .enumerate()
    {
        ui::create_element(
            &ctx,
            UiElement {
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
                image: None,
                is_input: false,
                cursor_pos: 0,
                scrollable_x: false,
                scrollable_y: false,
                scroll_x: 0.0,
                scroll_y: 0.0,
                visible: true,
                ..Default::default()
            },
        );
    }

    // Primary button
    ui::create_element(&ctx, button_element(BUTTON_BG));
}

#[reducer(on = "graphics.frametick.update")]
pub fn on_frame<Caps>(ctx: ReducerContext<Caps>, _prev: FrameTick, _tick: FrameTick)
where
    Caps: CanRead<ui::UiElement>
        + CanUpdate<ui::UiElement>
        + CanRead<ui::InputFocus>
        + CanRead<MouseState>,
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
    let hovered = is_over_button(mx, my);
    let bg = if hovered { BUTTON_HOVER_BG } else { BUTTON_BG };
    ui::update_element(&ctx, button_element(bg));

    // Lay out + draw our UI tree into our own layer/surface.
    ui::render(&ctx);
}

fn button_element(bg: (f32, f32, f32, f32)) -> UiElement {
    UiElement {
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

// Placeholder: approximate button region.
// TODO: replace with a bounds query once the UI engine exposes one.
fn is_over_button(mx: f32, my: f32) -> bool {
    let _ = (mx, my);
    false
}
