use crate::bindings::agar_server_example::agar_server::{
    DeadPlayer, Food, HasDeadPlayerHandle, HasFoodHandle, HasPlayerHandle, Player,
};
use crate::bindings::graphics::{Color, Vec2};
use crate::bindings::{agar_server_example::*, graphics::*, input::*, *};
use crate::death::{UI_DEAD_BTN, handle_dead_input, show_dead_screen};
use crate::hud::update_hud;
use crate::input::handle_ingame_input;
use crate::lobby::{UI_LOBBY_BTN, handle_lobby_input, set_button_hover};
use crate::tables::*;
use crate::ui::{self, UiElement};
use interstice_sdk::*;
use interstice_ui::FontStyle;
use interstice_ui::theme::{self as th, Rgba};

// The arena, drawn in the brand (brand/BRAND.md §8, Agar): an ink field with
// a hairline grid, your cell lit amber, everyone else in the quiet
// categorical hues, pellets in gray. The camera eases after your cell, so
// joining swoops in from the lobby's view and growing zooms out smoothly.

// ── Layers ────────────────────────────────────────────────────────────────────

const LAYER_BG: &str = "agar.bg";
const LAYER_WORLD: &str = "agar.world";
const LAYER_NAMES: &str = "agar.names";
const GLOW_TEX: &str = "agar_glow";

// ── Camera ────────────────────────────────────────────────────────────────────

pub const BASE_ZOOM: f32 = 1.0;
const ZOOM_SCALE: f32 = 0.012;
const MIN_ZOOM: f32 = 0.15;
const ZOOM_LERP: f32 = 0.06;
const FOLLOW_LERP: f32 = 0.18;

const WORLD_SIZE: f32 = 2_000.0;
/// Grid spacing in world units at zoom 1 (= screen px).
const GRID_SPACING: f32 = 48.0;

pub fn init_layers<Caps>(ctx: &ReducerContext<Caps>)
where
    Caps: CanInsert<ClientState>,
{
    // Layers above z=0: the graphics "default" layer (z=0, clear) clears the canvas
    // first, then our layers composite on top.
    let g = ctx.graphics();
    let _ = g.reducers.create_layer(LAYER_BG.to_string(), 1, false);
    let _ = g.reducers.create_layer(LAYER_WORLD.to_string(), 2, false);
    let _ = g.reducers.create_layer(LAYER_NAMES.to_string(), 3, false);
    // The UI layer (z=100) is created by `ui::install` in `init`.

    // Your cell's halo, baked once (the renderer has no blur).
    let n = 128;
    let _ = g.reducers.create_texture(
        GLOW_TEX.to_string(),
        TextureDescriptorInput {
            width: n,
            height: n,
            format: "rgba8unorm_srgb".to_string(),
            mip_levels: 1,
            sample_count: 1,
            usage: TextureUsageFlags {
                copy_src: false,
                copy_dst: true,
                texture_binding: true,
                storage_binding: false,
                render_attachment: false,
            },
        },
        Blob::new(&th::glow_rgba(n, th::AMBER)),
    );

    let _ = ctx.current.tables.clientstate().insert(ClientState {
        id: 0,
        state: GameState::Lobby,
        zoom: BASE_ZOOM,
        target_zoom: BASE_ZOOM,
        final_score: 0.0,
        cam_x: 0.0,
        cam_y: 0.0,
        was_down: false,
        hud_sig: 0,
        pending_join: None,
    });
}

/// Join as `name` as if typed in the lobby (scripting, tests):
/// `interstice call_reducer agar-client-example agar-client play Ada`.
#[reducer]
pub fn play<Caps>(ctx: ReducerContext<Caps>, name: String) -> Result<(), String>
where
    Caps: CanRead<ClientState> + CanUpdate<ClientState>,
{
    let Some(mut cs) = ctx.current.tables.clientstate().get(0)? else {
        return Err("agar-client: not loaded".into());
    };
    cs.pending_join = Some(name);
    ctx.current.tables.clientstate().update(cs)
}

#[reducer(on = "graphics.frametick.update")]
pub fn on_frame<Caps>(ctx: ReducerContext<Caps>, _prev: FrameTick, _tick: FrameTick) -> Result<(), String>
where
    Caps: CanRead<ClientState>
        + CanUpdate<ClientState>
        + CanRead<KeyState>
        + CanRead<Player>
        + CanRead<Food>
        + CanRead<DeadPlayer>
        + CanAll<UiElement>
        + CanAll<ui::UiPointer>
        + CanRead<ui::InputFocus>
        + CanInsert<ui::InputFocus>
        + CanUpdate<ui::InputFocus>,
{
    let Some(mut cs) = ctx.current.tables.clientstate().get(0)? else {
        return Ok(());
    };
    let before = (cs.was_down, cs.hud_sig, cs.state.clone(), cs.pending_join.clone());
    let g = ctx.graphics();
    // Our own surface: the swapchain standalone, a window inside a desktop.
    let (sw, sh) = match g.queries.surface_info().ok() {
        Some(s) if s.width > 0 && s.height > 0 => (s.width as f32, s.height as f32),
        _ => return Ok(()),
    };

    // Pointer: the element under it, and a click on this frame's press edge.
    let view = ctx.input().queries.input_view()?;
    let over = if view.pointer_inside { ui::element_at(&ctx, view.position) } else { None };
    let clicked = (view.left && !cs.was_down).then_some(()).and(over.as_deref());
    cs.was_down = view.left;

    match cs.state {
        GameState::Lobby => {
            render_world(&ctx, &mut cs, sw, sh)?;
            set_button_hover(&ctx, UI_LOBBY_BTN, over.as_deref() == Some(UI_LOBBY_BTN))?;
            handle_lobby_input(&ctx, &mut cs, clicked)?;
        }
        GameState::InGame => {
            // Check for own death.
            let my_id = ctx.current_node_id();
            let server = ctx.agar_server_example().agar_server();
            if server.tables.deadplayer().scan()?.iter().any(|d| d.id == my_id) {
                cs.final_score = server.tables.player().get(my_id)?.map(|p| p.radius).unwrap_or(cs.final_score);
                cs.state = GameState::Dead;
                ui::clear_focus(&ctx);
                ui::clear_elements(&ctx)?;
                show_dead_screen(&ctx, cs.final_score);
            } else {
                if let Some(me) = server.tables.player().get(my_id)? {
                    cs.final_score = me.radius;
                }
                handle_ingame_input(&ctx, sw, sh)?;
                update_hud(&ctx, &mut cs)?;
            }
            render_world(&ctx, &mut cs, sw, sh)?;
        }
        GameState::Dead => {
            render_world(&ctx, &mut cs, sw, sh)?;
            set_button_hover(&ctx, UI_DEAD_BTN, over.as_deref() == Some(UI_DEAD_BTN))?;
            handle_dead_input(&ctx, &mut cs, clicked)?;
        }
    }

    // Lay out + draw our UI tree (lobby / HUD / death) into our own layer/surface.
    ui::render(&ctx)?;

    // The camera moves every frame in game; otherwise write only on change.
    let after = (cs.was_down, cs.hud_sig, cs.state.clone(), cs.pending_join.clone());
    if cs.state == GameState::InGame || before != after {
        let _ = ctx.current.tables.clientstate().update(cs);
    }
    Ok(())
}

fn col(c: Rgba) -> Color {
    Color { r: c.0, g: c.1, b: c.2, a: c.3 }
}

/// A stable categorical colour for a player id.
fn player_colour(id: &str) -> Rgba {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in id.bytes() {
        h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
    th::categorical(h)
}

struct View {
    cam_x: f32,
    cam_y: f32,
    zoom: f32,
    sw: f32,
    sh: f32,
}

impl View {
    fn to_screen(&self, wx: f32, wy: f32) -> (f32, f32) {
        ((wx - self.cam_x) * self.zoom + self.sw * 0.5, (wy - self.cam_y) * self.zoom + self.sh * 0.5)
    }
}

fn render_world<Caps>(ctx: &ReducerContext<Caps>, cs: &mut ClientState, sw: f32, sh: f32) -> Result<(), String>
where
    Caps: CanRead<Player> + CanRead<Food>,
{
    let server = ctx.agar_server_example().agar_server();
    let mut players = server.tables.player().scan()?;
    let foods = server.tables.food().scan()?;
    let my_id = ctx.current_node_id();
    let me = if cs.state == GameState::InGame { players.iter().find(|p| p.id == my_id) } else { None };

    // Ease the camera after our cell (it holds still in the lobby and after death).
    if let Some(p) = me {
        cs.cam_x += (p.pos.x - cs.cam_x) * FOLLOW_LERP;
        cs.cam_y += (p.pos.y - cs.cam_y) * FOLLOW_LERP;
        cs.target_zoom = (BASE_ZOOM / (1.0 + p.radius * ZOOM_SCALE)).clamp(MIN_ZOOM, BASE_ZOOM);
    }
    cs.zoom += (cs.target_zoom - cs.zoom) * ZOOM_LERP;
    let v = View { cam_x: cs.cam_x, cam_y: cs.cam_y, zoom: cs.zoom, sw, sh };

    let g = ctx.graphics();
    let rect = |layer: &str, x: f32, y: f32, w: f32, h: f32, c: Rgba| {
        if w > 0.0 && h > 0.0 {
            let _ = g.reducers.draw_rect(layer.to_string(), Rect { x, y, w, h }, col(c), true, 0.0, None);
        }
    };

    // ── Arena: ground outside, ink inside, a hairline grid and edge ──────────
    rect(LAYER_BG, 0.0, 0.0, sw, sh, th::GROUND);
    let (x0, y0) = v.to_screen(-WORLD_SIZE, -WORLD_SIZE);
    let (x1, y1) = v.to_screen(WORLD_SIZE, WORLD_SIZE);
    let (cx0, cy0, cx1, cy1) = (x0.max(0.0), y0.max(0.0), x1.min(sw), y1.min(sh));
    rect(LAYER_BG, cx0, cy0, cx1 - cx0, cy1 - cy0, th::INK);

    // Coarsen the grid as we zoom out so lines stay ≥ 24 px apart.
    let mut spacing = GRID_SPACING;
    while spacing * v.zoom < 24.0 {
        spacing *= 2.0;
    }
    let grid = th::alpha(th::LINE, 0.5);
    let mut gx = ((v.cam_x - sw / (2.0 * v.zoom)) / spacing).floor() * spacing;
    while gx <= v.cam_x + sw / (2.0 * v.zoom) {
        if gx > -WORLD_SIZE && gx < WORLD_SIZE {
            let sx = v.to_screen(gx, 0.0).0.round();
            rect(LAYER_BG, sx, cy0, 1.0, cy1 - cy0, grid);
        }
        gx += spacing;
    }
    let mut gy = ((v.cam_y - sh / (2.0 * v.zoom)) / spacing).floor() * spacing;
    while gy <= v.cam_y + sh / (2.0 * v.zoom) {
        if gy > -WORLD_SIZE && gy < WORLD_SIZE {
            let sy = v.to_screen(0.0, gy).1.round();
            rect(LAYER_BG, cx0, sy, cx1 - cx0, 1.0, grid);
        }
        gy += spacing;
    }
    let _ = g.reducers.draw_rect(
        LAYER_BG.to_string(),
        Rect { x: x0.round(), y: y0.round(), w: (x1 - x0).round(), h: (y1 - y0).round() },
        col(th::alpha(th::GRAY, 0.5)),
        false,
        1.0,
        None,
    );

    // ── Pellets: gray, three shades so the field has some texture ────────────
    let mut batches: [(Vec<Vec2>, Vec<f32>); 3] = Default::default();
    let margin = 20.0;
    for food in &foods {
        let (x, y) = v.to_screen(food.pos.x, food.pos.y);
        if x < -margin || y < -margin || x > sw + margin || y > sh + margin {
            continue;
        }
        let b = &mut batches[(food.id % 3) as usize];
        b.0.push(Vec2 { x, y });
        b.1.push((food.radius * v.zoom).max(1.5));
    }
    for (i, (centers, radii)) in batches.into_iter().enumerate() {
        if !centers.is_empty() {
            let c = th::alpha(th::GRAY, [0.55, 0.75, 0.95][i]);
            let _ = g.reducers.draw_circles(LAYER_WORLD.to_string(), centers, radii, col(c), true, 0.0);
        }
    }

    // ── Cells: smaller first, so bigger ones cover them ─────────────────────
    players.sort_by(|a, b| a.radius.partial_cmp(&b.radius).unwrap_or(std::cmp::Ordering::Equal));
    for player in &players {
        let (x, y) = v.to_screen(player.pos.x, player.pos.y);
        let r = player.radius * v.zoom;
        if x + r < 0.0 || y + r < 0.0 || x - r > sw || y - r > sh {
            continue;
        }
        let is_me = player.id == my_id && cs.state == GameState::InGame;
        let centre = Vec2 { x, y };
        let (fill, rim) = if is_me {
            // The lit cell: a soft amber halo, then the cell with a core rim.
            let halo = r * 2.4;
            let _ = g.reducers.draw_image(
                LAYER_WORLD.to_string(),
                GLOW_TEX.to_string(),
                Rect { x: x - halo, y: y - halo, w: 2.0 * halo, h: 2.0 * halo },
                col((1.0, 1.0, 1.0, 0.7)),
                Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 },
            );
            (th::AMBER, th::alpha(th::CORE, 0.9))
        } else {
            let c = player_colour(&player.id);
            (c, th::alpha(th::mix(c, th::IVORY, 0.35), 0.9))
        };
        let _ = g.reducers.draw_circle(LAYER_WORLD.to_string(), Vec2 { x, y }, r, col(fill), true, 0.0);
        let _ = g.reducers.draw_circle(LAYER_WORLD.to_string(), centre, (r - 1.0).max(1.0), col(rim), false, if is_me { 2.0 } else { 1.5 });

        // Name (and mass) in Space Grotesk: inside the cell when it's big
        // enough, above it otherwise.
        let mass = format!("{}", player.radius as u32);
        let name_size = (r * 0.34).clamp(12.0, 22.0).round();
        let name_w = interstice_ui::text_width(&player.name, name_size, FontStyle::MEDIUM);
        let inside = r >= 28.0 && name_w < r * 1.7;
        let text = |s: String, x: f32, y: f32, size: f32, c: Rgba, style: FontStyle| {
            let pos = Vec2 { x: x.round(), y: y.round() };
            let _ = g.reducers.draw_text(LAYER_NAMES.to_string(), s, pos, size, col(c), style.selector());
        };
        if inside {
            let mass_size = (name_size * 0.7).round().max(11.0);
            let mass_w = interstice_ui::text_width(&mass, mass_size, FontStyle::MONO);
            let block = name_size * 1.25 + mass_size;
            let top = y - block / 2.0;
            let ink = th::alpha(th::INK, 0.88);
            text(player.name.clone(), x - name_w / 2.0, top, name_size, ink, FontStyle::MEDIUM);
            text(mass, x - mass_w / 2.0, top + name_size * 1.25, mass_size, th::alpha(th::INK, 0.6), FontStyle::MONO);
        } else {
            let c = if is_me { th::IVORY } else { th::MUTED };
            text(player.name.clone(), x - name_w / 2.0, y - r - name_size - 6.0, name_size, c, FontStyle::MEDIUM);
        }
    }
    Ok(())
}
