use crate::lobby::build_lobby_ui;
use crate::style::*;
use crate::tables::*;
use crate::ui::{self, Size, UiElement};
use crate::bindings::input::*;
use interstice_sdk::{key_code::KeyCode, *};
use interstice_ui::theme as th;

const UI_DEAD_ROOT: &str = "dead_root";
const UI_DEAD_CARD: &str = "dead_card";
pub const UI_DEAD_BTN: &str = "dead_play_btn";

pub fn show_dead_screen<Caps>(ctx: &ReducerContext<Caps>, final_score: f32)
where
    Caps: CanInsert<UiElement> + CanUpdate<UiElement>,
{
    let el = |e: UiElement| ui::create_element(ctx, e);
    el(centred_root(UI_DEAD_ROOT, 20, th::SCRIM));
    el(panel(UI_DEAD_CARD, UI_DEAD_ROOT, 0, Size::Fixed(340.0), 32.0, 12.0));
    el(label("dead_kicker", UI_DEAD_CARD, 0, "Game over", th::DANGER));
    el(text("dead_msg", UI_DEAD_CARD, 1, "You were eaten", th::TEXT_HEADING, th::IVORY));
    el(rule("dead_rule", UI_DEAD_CARD, 2));
    el(label("dead_score_label", UI_DEAD_CARD, 3, "Final mass", th::GRAY));
    el(mono("dead_score", UI_DEAD_CARD, 4, &format!("{}", final_score as u32), th::TEXT_DISPLAY, th::AMBER));
    el(gap("dead_gap", UI_DEAD_CARD, 5, 4.0));
    el(primary_button(UI_DEAD_BTN, UI_DEAD_CARD, 6, "Play again", false));
    el(label("dead_hint", UI_DEAD_CARD, 7, "Enter to go back", th::DIM));
}

pub fn handle_dead_input<Caps>(
    ctx: &ReducerContext<Caps>,
    cs: &mut ClientState,
    clicked: Option<&str>,
) -> Result<(), String>
where
    Caps: CanRead<KeyState>
        + CanRead<ClientState>
        + CanInsert<UiElement>
        + CanUpdate<UiElement>
        + CanRead<UiElement>
        + CanDelete<UiElement>
        + CanInsert<ui::InputFocus>
        + CanUpdate<ui::InputFocus>,
{
    let mut go = clicked == Some(UI_DEAD_BTN) || cs.pending_join.is_some();
    // Keys are ours only while we have keyboard focus (DESIGN §12.2).
    if !go && ctx.input().queries.input_view()?.keyboard_focus {
        let key_states = ctx.input().tables.keystate().scan()?;
        go = key_states
            .iter()
            .any(|k| k.pressed && (k.code == KeyCode::Enter as u32 || k.code == KeyCode::NumpadEnter as u32));
    }
    if go {
        ui::clear_focus(ctx);
        ui::clear_elements(ctx)?;
        build_lobby_ui(ctx);
        cs.state = GameState::Lobby;
    }
    Ok(())
}
