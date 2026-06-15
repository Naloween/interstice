use crate::ui::{self, LayoutDirection, Size, TextWrap, UiElement};
use crate::{bindings::input::*, lobby::build_lobby_ui, tables::*};
use interstice_sdk::{key_code::KeyCode, *};

const UI_DEAD_ROOT: &str = "dead_root";
const UI_DEAD_CARD: &str = "dead_card";
const UI_DEAD_MSG: &str = "dead_msg";
const UI_DEAD_SCORE: &str = "dead_score";
const UI_DEAD_BTN: &str = "dead_play_btn";

pub fn show_dead_screen<Caps>(ctx: &ReducerContext<Caps>, final_score: f32)
where
    Caps: CanInsert<UiElement>,
{
    let none = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let text_col = (0.92f32, 0.92f32, 0.95f32, 1.0f32);
    let card_bg = (0.14f32, 0.14f32, 0.18f32, 1.0f32);
    let btn_bg = (0.27f32, 0.47f32, 0.87f32, 1.0f32);

    let mk = |id: &str,
              parent: Option<&str>,
              order: u32,
              w: Size,
              h: Size,
              gap: f32,
              pad: f32,
              mar: f32,
              bg: (f32, f32, f32, f32),
              cr: f32,
              text: Option<String>,
              ts: f32,
              tc: (f32, f32, f32, f32)| {
        ui::create_element(ctx, UiElement {
            id: id.into(),
            parent: parent.map(|s| s.into()),
            order,
            width: w,
            height: h,
            layout_direction: LayoutDirection::Column,
            gap,
            padding: pad,
            margin: mar,
            background_color: bg,
            corner_radius: cr,
            border_width: 0.0,
            border_color: none,
            text,
            text_size: ts,
            text_color: tc,
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

    mk(
        UI_DEAD_ROOT,
        None,
        20,
        Size::Grow,
        Size::Grow,
        0.0,
        0.0,
        0.0,
        (0.0, 0.0, 0.0, 0.6),
        0.0,
        None,
        0.0,
        none,
    );

    mk(
        UI_DEAD_CARD,
        Some(UI_DEAD_ROOT),
        0,
        Size::Fixed(300.0),
        Size::Fit,
        20.0,
        32.0,
        0.0,
        card_bg,
        12.0,
        None,
        0.0,
        none,
    );

    mk(
        UI_DEAD_MSG,
        Some(UI_DEAD_CARD),
        0,
        Size::Grow,
        Size::Fit,
        0.0,
        0.0,
        0.0,
        none,
        0.0,
        Some("You were eaten!".into()),
        22.0,
        (0.95, 0.4, 0.4, 1.0),
    );

    mk(
        UI_DEAD_SCORE,
        Some(UI_DEAD_CARD),
        1,
        Size::Grow,
        Size::Fit,
        0.0,
        0.0,
        0.0,
        none,
        0.0,
        Some(format!("Final score: {}", final_score as u32)),
        14.0,
        text_col,
    );

    mk(
        UI_DEAD_BTN,
        Some(UI_DEAD_CARD),
        2,
        Size::Grow,
        Size::Fixed(40.0),
        0.0,
        8.0,
        0.0,
        btn_bg,
        8.0,
        Some("Play again".into()),
        16.0,
        (1.0, 1.0, 1.0, 1.0),
    );
}

pub fn handle_dead_input<Caps>(ctx: &ReducerContext<Caps>, cs: &mut ClientState)
where
    Caps: CanRead<KeyState>
        + CanRead<ClientState>
        + CanInsert<UiElement>
        + CanRead<UiElement>
        + CanDelete<UiElement>
        + CanInsert<ui::InputFocus>
        + CanUpdate<ui::InputFocus>,
{
    let key_states = ctx.input().tables.keystate().scan();
    let enter = key_states.iter().any(|k| {
        k.pressed && (k.code == KeyCode::Enter as u32 || k.code == KeyCode::NumpadEnter as u32)
    });
    if enter {
        ui::clear_focus(ctx);
        ui::clear_elements(ctx);
        build_lobby_ui(ctx);
        cs.state = GameState::Lobby;
    }
}
