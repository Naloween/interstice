use crate::bindings::input::*;
use crate::bindings::{agar_server_example::*, *};
use crate::hud::build_hud_ui;
use crate::render::BASE_ZOOM;
use crate::style::*;
use crate::tables::*;
use crate::ui::{self, HasUiElementEditHandle, UiElement};
use interstice_sdk::key_code::KeyCode;
use interstice_sdk::*;
use interstice_ui::theme as th;

const UI_LOBBY_ROOT: &str = "lobby_root";
const UI_LOBBY_CARD: &str = "lobby_card";
const UI_LOBBY_INPUT: &str = "lobby_name_input";
pub const UI_LOBBY_BTN: &str = "lobby_play_btn";

pub fn build_lobby_ui<Caps>(ctx: &ReducerContext<Caps>)
where
    Caps: CanInsert<UiElement> + CanUpdate<UiElement> + CanInsert<ui::InputFocus> + CanUpdate<ui::InputFocus>,
{
    let el = |e: UiElement| ui::create_element(ctx, e);
    // Transparent root: the arena is drawn behind it, still and dimmed.
    el(centred_root(UI_LOBBY_ROOT, 0, th::alpha(th::INK, 0.55)));
    el(panel(UI_LOBBY_CARD, UI_LOBBY_ROOT, 0, crate::ui::Size::Fixed(380.0), 32.0, 12.0));
    el(label("lobby_kicker", UI_LOBBY_CARD, 0, "Interstice example", th::GRAY));
    el(text("lobby_title", UI_LOBBY_CARD, 1, "Agar", th::TEXT_DISPLAY, th::IVORY));
    el(text(
        "lobby_sub",
        UI_LOBBY_CARD,
        2,
        "Eat pellets, grow, and stay away from bigger cells. Every cell is a row in agar-server's Player table.",
        th::TEXT_BODY,
        th::MUTED,
    ));
    el(gap("lobby_gap", UI_LOBBY_CARD, 3, 6.0));
    el(label("lobby_name_label", UI_LOBBY_CARD, 4, "Your name", th::GRAY));
    el(input(UI_LOBBY_INPUT, UI_LOBBY_CARD, 5, ""));
    el(primary_button(UI_LOBBY_BTN, UI_LOBBY_CARD, 6, "Play", false));
    el(label("lobby_hint", UI_LOBBY_CARD, 7, "Enter to play · steer with the mouse", th::DIM));
    ui::set_focus(ctx, UI_LOBBY_INPUT);
}

/// Hover feedback on the lobby's or the death screen's primary button.
pub fn set_button_hover<Caps>(ctx: &ReducerContext<Caps>, id: &str, hover: bool) -> Result<(), String>
where
    Caps: CanRead<UiElement> + CanUpdate<UiElement>,
{
    if let Some(mut b) = ctx.current.tables.uielement().get(id.to_string())? {
        let bg = if hover { th::AMBER_HI } else { th::AMBER };
        if b.background_color != bg {
            b.background_color = bg;
            ctx.current.tables.uielement().update(b)?;
        }
    }
    Ok(())
}

pub fn handle_lobby_input<Caps>(
    ctx: &ReducerContext<Caps>,
    cs: &mut ClientState,
    clicked: Option<&str>,
) -> Result<(), String>
where
    Caps: CanRead<ClientState>
        + CanUpdate<ClientState>
        + CanRead<KeyState>
        + CanAll<UiElement>
        + CanInsert<ui::InputFocus>
        + CanUpdate<ui::InputFocus>,
{
    if let Some(name) = cs.pending_join.take() {
        return start_game(ctx, cs, Some(name));
    }
    if clicked == Some(UI_LOBBY_BTN) {
        return start_game(ctx, cs, None);
    }
    // Keys are ours only while we have keyboard focus (DESIGN §12.2).
    if !ctx.input().queries.input_view()?.keyboard_focus {
        return Ok(());
    }
    let key_states = ctx.input().tables.keystate().scan()?;
    let enter_pressed = key_states
        .iter()
        .any(|k| k.pressed && (k.code == KeyCode::Enter as u32 || k.code == KeyCode::NumpadEnter as u32));
    if enter_pressed {
        start_game(ctx, cs, None)?;
    }
    Ok(())
}

fn start_game<Caps>(ctx: &ReducerContext<Caps>, cs: &mut ClientState, name: Option<String>) -> Result<(), String>
where
    Caps: CanRead<ClientState>
        + CanUpdate<ClientState>
        + CanAll<UiElement>
        + CanInsert<ui::InputFocus>
        + CanUpdate<ui::InputFocus>,
{
    // The typed name, from our own UI input element.
    let name = name
        .or_else(|| {
            ctx.current.tables.uielement().get(UI_LOBBY_INPUT.to_string()).ok().flatten().and_then(|el| el.text)
        })
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "Player".to_string());

    let server = ctx.agar_server_example().agar_server();
    if let Err(err) = server.reducers.join(name) {
        ctx.log(&format!("join failed: {err}"));
        return Ok(());
    }

    // Clear focus BEFORE clearing elements so the render pass doesn't try to
    // update the focused (now-deleted) lobby input element on subsequent frames.
    ui::clear_focus(ctx);
    ui::clear_elements(ctx)?;
    build_hud_ui(ctx);

    cs.state = GameState::InGame;
    cs.zoom = BASE_ZOOM;
    cs.target_zoom = BASE_ZOOM;
    cs.hud_sig = 0;
    Ok(())
}
