mod death;
mod hud;
mod input;
mod lobby;
mod render;
mod tables;

use crate::tables::ClientState;
use crate::{lobby::build_lobby_ui, render::init_layers};
use interstice_sdk::*;

interstice_module!(
    visibility: Public,
    replicated_tables: [
        "agar-server-example.agar-server.player",
        "agar-server-example.agar-server.food",
        "agar-server-example.agar-server.deadplayer",
    ]
);

// Module-local UI subsystem (own tables, helpers, render, key reducer) wired to
// this module's own graphics/input bindings — draws into our OWN layer so the
// desktop compositor can route us to our own surface.
interstice_ui::ui_subsystem!();

// ── Load ──────────────────────────────────────────────────────────────────────

#[reducer(on = "load")]
pub fn init<Caps>(ctx: ReducerContext<Caps>)
where
    Caps: CanInsert<ClientState>
        + CanInsert<ui::InputFocus>
        + CanInsert<ui::UiElement>
        + CanRead<ui::InputFocus>
        + CanUpdate<ui::InputFocus>,
{
    init_layers(&ctx);
    ui::install(&ctx);
    build_lobby_ui(&ctx);
}
