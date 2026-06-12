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

// ── Load ──────────────────────────────────────────────────────────────────────

#[reducer(on = "load")]
pub fn init<Caps>(ctx: ReducerContext<Caps>)
where
    Caps: CanInsert<ClientState>,
{
    init_layers(&ctx);
    build_lobby_ui(&ctx);
}
