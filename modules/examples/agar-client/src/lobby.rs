use crate::bindings::input::*;
use crate::bindings::{agar_server_example::*, *};
use crate::hud::build_hud_ui;
use crate::render::BASE_ZOOM;
use crate::tables::*;
use crate::ui::{self, HasUiElementEditHandle, LayoutDirection, Size, TextWrap, UiElement};
use interstice_sdk::key_code::KeyCode;
use interstice_sdk::*;

const UI_LOBBY_ROOT: &str = "lobby_root";
const UI_LOBBY_CARD: &str = "lobby_card";
const UI_LOBBY_TITLE: &str = "lobby_title";
const UI_LOBBY_INPUT: &str = "lobby_name_input";
const UI_LOBBY_BTN: &str = "lobby_play_btn";

pub fn build_lobby_ui<Caps>(ctx: &ReducerContext<Caps>)
where
    Caps: CanInsert<UiElement>
        + CanUpdate<UiElement>
        + CanInsert<ui::InputFocus>
        + CanUpdate<ui::InputFocus>,
{
    let none = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let text_col = (0.92f32, 0.92f32, 0.95f32, 1.0f32);
    let muted = (0.55f32, 0.55f32, 0.62f32, 1.0f32);
    let card_bg = (0.14f32, 0.14f32, 0.18f32, 1.0f32);
    let input_bg = (0.10f32, 0.10f32, 0.13f32, 1.0f32);
    let btn_bg = (0.27f32, 0.47f32, 0.87f32, 1.0f32);

    let spacer = |id: &str, parent: &str, order: u32| {
        ui::create_element(ctx, UiElement {
            id: id.into(),
            parent: Some(parent.into()),
            order,
            width: Size::Grow,
            height: Size::Grow,
            layout_direction: LayoutDirection::Row,
            gap: 0.0,
            padding: 0.0,
            margin: 0.0,
            background_color: none,
            corner_radius: 0.0,
            border_width: 0.0,
            border_color: none,
            text: None,
            text_size: 0.0,
            text_color: none,
            text_wrap: TextWrap::None,
            is_input: false,
            cursor_pos: 0,
            scrollable_x: false,
            scrollable_y: false,
            scroll_x: 0.0,
            scroll_y: 0.0,
            visible: true,
        });
    };

    // Full-screen dark root (Column — vertical centering via spacers).
    ui::create_element(ctx, UiElement {
        id: UI_LOBBY_ROOT.into(),
        parent: None,
        order: 0,
        width: Size::Grow,
        height: Size::Grow,
        layout_direction: LayoutDirection::Column,
        gap: 0.0,
        padding: 0.0,
        margin: 0.0,
        background_color: (0.07, 0.07, 0.10, 1.0),
        corner_radius: 0.0,
        border_width: 0.0,
        border_color: none,
        text: None,
        text_size: 0.0,
        text_color: none,
        text_wrap: TextWrap::None,
        is_input: false,
        cursor_pos: 0,
        scrollable_x: false,
        scrollable_y: false,
        scroll_x: 0.0,
        scroll_y: 0.0,
        visible: true,
    });

    // Top spacer — pushes center row down.
    spacer("lobby_v_top", UI_LOBBY_ROOT, 0);

    // Center row — horizontal centering via spacers.
    ui::create_element(ctx, UiElement {
        id: "lobby_center_row".into(),
        parent: Some(UI_LOBBY_ROOT.into()),
        order: 1,
        width: Size::Grow,
        height: Size::Fit,
        layout_direction: LayoutDirection::Row,
        gap: 0.0,
        padding: 0.0,
        margin: 0.0,
        background_color: none,
        corner_radius: 0.0,
        border_width: 0.0,
        border_color: none,
        text: None,
        text_size: 0.0,
        text_color: none,
        text_wrap: TextWrap::None,
        is_input: false,
        cursor_pos: 0,
        scrollable_x: false,
        scrollable_y: false,
        scroll_x: 0.0,
        scroll_y: 0.0,
        visible: true,
    });

    // Left spacer — pushes card right.
    spacer("lobby_h_left", "lobby_center_row", 0);

    // Card itself.
    ui::create_element(ctx, UiElement {
        id: UI_LOBBY_CARD.into(),
        parent: Some("lobby_center_row".into()),
        order: 1,
        width: Size::Fixed(360.0),
        height: Size::Fit,
        layout_direction: LayoutDirection::Column,
        gap: 16.0,
        padding: 28.0,
        margin: 0.0,
        background_color: card_bg,
        corner_radius: 12.0,
        border_width: 1.0,
        border_color: (0.28, 0.28, 0.34, 1.0),
        text: None,
        text_size: 0.0,
        text_color: none,
        text_wrap: TextWrap::None,
        is_input: false,
        cursor_pos: 0,
        scrollable_x: false,
        scrollable_y: false,
        scroll_x: 0.0,
        scroll_y: 0.0,
        visible: true,
    });

    // Right spacer — pushes card left.
    spacer("lobby_h_right", "lobby_center_row", 2);

    // Bottom spacer — pushes center row up.
    spacer("lobby_v_bot", UI_LOBBY_ROOT, 2);

    // Card contents.
    ui::create_element(ctx, UiElement {
        id: UI_LOBBY_TITLE.into(),
        parent: Some(UI_LOBBY_CARD.into()),
        order: 0,
        width: Size::Grow,
        height: Size::Fit,
        layout_direction: LayoutDirection::Row,
        gap: 0.0,
        padding: 0.0,
        margin: 0.0,
        background_color: none,
        corner_radius: 0.0,
        border_width: 0.0,
        border_color: none,
        text: Some("agar.io".into()),
        text_size: 28.0,
        text_color: text_col,
        text_wrap: TextWrap::None,
        is_input: false,
        cursor_pos: 0,
        scrollable_x: false,
        scrollable_y: false,
        scroll_x: 0.0,
        scroll_y: 0.0,
        visible: true,
    });
    ui::create_element(ctx, UiElement {
        id: "lobby_sub".into(),
        parent: Some(UI_LOBBY_CARD.into()),
        order: 1,
        width: Size::Grow,
        height: Size::Fit,
        layout_direction: LayoutDirection::Row,
        gap: 0.0,
        padding: 0.0,
        margin: 0.0,
        background_color: none,
        corner_radius: 0.0,
        border_width: 0.0,
        border_color: none,
        text: Some("Enter your name to play".into()),
        text_size: 13.0,
        text_color: muted,
        text_wrap: TextWrap::None,
        is_input: false,
        cursor_pos: 0,
        scrollable_x: false,
        scrollable_y: false,
        scroll_x: 0.0,
        scroll_y: 0.0,
        visible: true,
    });
    ui::create_element(ctx, UiElement {
        id: UI_LOBBY_INPUT.into(),
        parent: Some(UI_LOBBY_CARD.into()),
        order: 2,
        width: Size::Grow,
        height: Size::Fixed(36.0),
        layout_direction: LayoutDirection::Row,
        gap: 0.0,
        padding: 8.0,
        margin: 0.0,
        background_color: input_bg,
        corner_radius: 6.0,
        border_width: 1.0,
        border_color: (0.28, 0.28, 0.34, 1.0),
        text: Some(String::new()),
        text_size: 14.0,
        text_color: text_col,
        text_wrap: TextWrap::None,
        is_input: true,
        cursor_pos: 0,
        scrollable_x: false,
        scrollable_y: false,
        scroll_x: 0.0,
        scroll_y: 0.0,
        visible: true,
    });
    ui::create_element(ctx, UiElement {
        id: UI_LOBBY_BTN.into(),
        parent: Some(UI_LOBBY_CARD.into()),
        order: 3,
        width: Size::Grow,
        height: Size::Fixed(40.0),
        layout_direction: LayoutDirection::Row,
        gap: 0.0,
        padding: 8.0,
        margin: 0.0,
        background_color: btn_bg,
        corner_radius: 8.0,
        border_width: 0.0,
        border_color: none,
        text: Some("Play".into()),
        text_size: 16.0,
        text_color: (1.0, 1.0, 1.0, 1.0),
        text_wrap: TextWrap::None,
        is_input: false,
        cursor_pos: 0,
        scrollable_x: false,
        scrollable_y: false,
        scroll_x: 0.0,
        scroll_y: 0.0,
        visible: true,
    });

    ui::set_focus(ctx, UI_LOBBY_INPUT);
}

pub fn handle_lobby_input<Caps>(ctx: &ReducerContext<Caps>, cs: &mut ClientState, sw: f32, sh: f32)
where
    Caps: CanRead<ClientState>
        + CanUpdate<ClientState>
        + CanRead<KeyState>
        + CanRead<UiElement>
        + CanInsert<UiElement>
        + CanUpdate<UiElement>
        + CanDelete<UiElement>
        + CanInsert<ui::InputFocus>
        + CanUpdate<ui::InputFocus>,
{
    // Check Enter or NumpadEnter to submit.
    let key_states = ctx.input().tables.keystate().scan();
    let enter_pressed = key_states.iter().any(|k| {
        k.pressed && (k.code == KeyCode::Enter as u32 || k.code == KeyCode::NumpadEnter as u32)
    });
    if enter_pressed {
        start_game(ctx, cs, sw, sh);
    }
}

fn start_game<Caps>(ctx: &ReducerContext<Caps>, cs: &mut ClientState, _sw: f32, _sh: f32)
where
    Caps: CanRead<ClientState>
        + CanUpdate<ClientState>
        + CanRead<UiElement>
        + CanInsert<UiElement>
        + CanUpdate<UiElement>
        + CanDelete<UiElement>
        + CanInsert<ui::InputFocus>
        + CanUpdate<ui::InputFocus>,
{
    // Read the typed name directly from our own UI input element.
    let name = ctx
        .current
        .tables
        .uielement()
        .get(UI_LOBBY_INPUT.to_string())
        .and_then(|el| el.text)
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "Player".to_string());

    let server = ctx.agar_server_example().agar_server();
    if let Err(err) = server.reducers.join(name) {
        ctx.log(&format!("join failed: {err}"));
        return;
    }

    // Clear focus BEFORE clearing elements so the render pass doesn't try to
    // update the focused (now-deleted) lobby input element on subsequent frames.
    ui::clear_focus(ctx);
    ui::clear_elements(ctx);
    build_hud_ui(ctx);

    cs.state = GameState::InGame;
    cs.zoom = BASE_ZOOM;
    cs.target_zoom = BASE_ZOOM;
}
