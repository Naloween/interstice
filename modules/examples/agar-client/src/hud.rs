use crate::bindings::agar_server_example::{agar_server::*, *};
use crate::bindings::*;
use crate::style::*;
use crate::tables::ClientState;
use crate::ui::{self, AlignItems, JustifyContent, LayoutDirection, Size, UiElement};
use interstice_sdk::*;
use interstice_ui::theme as th;

const LB_ROWS: usize = 8;
const LB_ROOT: &str = "hud_lb";

/// The HUD panels sit over the arena, slightly see-through.
fn hud_panel(id: &str, parent: &str, order: u32, w: Size) -> UiElement {
    UiElement { background_color: th::alpha(th::CHARCOAL, 0.88), corner_radius: th::RADIUS_WINDOW, ..panel(id, parent, order, w, 14.0, 6.0) }
}

pub fn build_hud_ui<Caps>(ctx: &ReducerContext<Caps>)
where
    Caps: CanInsert<UiElement> + CanUpdate<UiElement>,
{
    let el = |e: UiElement| ui::create_element(ctx, e);
    el(UiElement {
        padding: 16.0,
        align_items: AlignItems::Start,
        ..boxed("hud_root", None, 10, LayoutDirection::Row, Size::Grow, Size::Fit)
    });

    // Top left: your mass.
    el(hud_panel("hud_mass", "hud_root", 0, Size::Fixed(132.0)));
    el(label("hud_mass_label", "hud_mass", 0, "Mass", th::GRAY));
    el(mono("hud_score", "hud_mass", 1, "18", th::TEXT_HEADING, th::AMBER));
    el(boxed("hud_spacer", Some("hud_root"), 1, LayoutDirection::Row, Size::Grow, Size::Fixed(1.0)));

    // Top right: the leaderboard. Rows are pre-created slots that `update_hud`
    // only rewrites (never delete + recreate in one run: the insert would hit
    // the committed row).
    el(hud_panel(LB_ROOT, "hud_root", 2, Size::Fixed(240.0)));
    el(UiElement { gap: 8.0, align_items: AlignItems::Center, ..boxed("hud_lb_head", Some(LB_ROOT), 0, LayoutDirection::Row, Size::Grow, Size::Fit) });
    el(label("hud_lb_title", "hud_lb_head", 0, "Leaderboard", th::GRAY));
    el(UiElement { text_align: 1.0, ..mono("hud_lb_count", "hud_lb_head", 1, "", th::TEXT_CAPTION, th::DIM) });
    el(rule("hud_lb_rule", LB_ROOT, 1));
    for i in 0..LB_ROWS {
        for e in lb_row(i, None) {
            el(e);
        }
    }

    // Bottom left: how to play.
    el(UiElement {
        padding: 18.0,
        justify_content: JustifyContent::End,
        ..boxed("hud_hint_root", None, 11, LayoutDirection::Column, Size::Grow, Size::Grow)
    });
    el(label("hud_hint", "hud_hint_root", 0, "Steer with the mouse or WASD", th::GRAY));
}

/// One leaderboard slot: (rank, name, score, is you), or hidden.
fn lb_row(i: usize, row: Option<(&str, u32, bool)>) -> Vec<UiElement> {
    let id = format!("hud_lb_{i}");
    let me = row.is_some_and(|r| r.2);
    let visible = row.is_some();
    let (name, score) = row.map_or(("", 0), |r| (r.0, r.1));
    let mut els = vec![
        UiElement {
            height: Size::Fixed(24.0),
            align_items: AlignItems::Center,
            gap: 8.0,
            padding_sides: Some((0.0, 8.0, 0.0, 0.0)),
            background_color: if me { th::SELECTED_BG } else { CLEAR },
            corner_radius: th::RADIUS_CHIP,
            visible,
            ..boxed(&id, Some(LB_ROOT), (i + 2) as u32, LayoutDirection::Row, Size::Grow, Size::Fixed(24.0))
        },
        UiElement {
            background_color: if me { th::AMBER } else { CLEAR },
            ..boxed(&format!("{id}_bar"), Some(&id), 0, LayoutDirection::Row, Size::Fixed(2.0), Size::Fixed(14.0))
        },
        UiElement { width: Size::Fixed(16.0), ..mono(&format!("{id}_rank"), &id, 1, &format!("{}", i + 1), th::TEXT_CAPTION, th::GRAY) },
        UiElement {
            text_wrap: crate::ui::TextWrap::None,
            ..text(&format!("{id}_name"), &id, 2, name, th::TEXT_BODY, if me { th::IVORY } else { th::MUTED })
        },
        UiElement {
            width: Size::Fixed(44.0),
            text_align: 1.0,
            ..mono(&format!("{id}_score"), &id, 3, &format!("{score}"), th::TEXT_SMALL, if me { th::AMBER } else { th::GRAY })
        },
    ];
    for e in els.iter_mut().skip(1) {
        e.visible = visible;
    }
    els
}

pub fn update_hud<Caps>(ctx: &ReducerContext<Caps>, cs: &mut ClientState) -> Result<(), String>
where
    Caps: CanRead<Player> + CanUpdate<UiElement>,
{
    let my_id = ctx.current_node_id();
    let mut players: Vec<Player> = ctx.agar_server_example().agar_server().tables.player().scan()?;
    players.sort_by(|a, b| b.radius.partial_cmp(&a.radius).unwrap_or(std::cmp::Ordering::Equal));
    let mine = players.iter().find(|p| p.id == my_id).map(|p| p.radius as u32);

    // Rewrite the HUD only when what it shows changes.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for b in bytes {
            h = (h ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    eat(&mine.unwrap_or(0).to_le_bytes());
    eat(&(players.len() as u32).to_le_bytes());
    for p in players.iter().take(LB_ROWS) {
        eat(p.id.as_bytes());
        eat(p.name.as_bytes());
        eat(&(p.radius as u32).to_le_bytes());
    }
    if h == cs.hud_sig {
        return Ok(());
    }
    cs.hud_sig = h;

    if let Some(m) = mine {
        ui::update_element(ctx, mono("hud_score", "hud_mass", 1, &format!("{m}"), th::TEXT_HEADING, th::AMBER));
    }
    let count = format!("{} {}", players.len(), if players.len() == 1 { "player" } else { "players" });
    ui::update_element(ctx, UiElement { text_align: 1.0, ..mono("hud_lb_count", "hud_lb_head", 1, &count, th::TEXT_CAPTION, th::DIM) });
    for i in 0..LB_ROWS {
        let row = players.get(i).map(|p| (p.name.as_str(), p.radius as u32, p.id == my_id));
        for e in lb_row(i, row) {
            ui::update_element(ctx, e);
        }
    }
    Ok(())
}
