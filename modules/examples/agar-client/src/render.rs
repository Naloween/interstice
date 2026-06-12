use crate::bindings::agar_server_example::agar_server::{Vec2 as Vec2Server, *};
use crate::bindings::graphics::{Color, Vec2};
use crate::bindings::{agar_server_example::*, graphics::*, input::*, ui::*, *};
use crate::death::{handle_dead_input, show_dead_screen};
use crate::hud::update_hud;
use crate::input::handle_ingame_input;
use crate::lobby::handle_lobby_input;
use crate::tables::*;
use interstice_sdk::*;

// ── Layers ────────────────────────────────────────────────────────────────────

const LAYER_BG: &str = "agar.bg";
const LAYER_WORLD: &str = "agar.world";
const LAYER_NAMES: &str = "agar.names";
const LAYER_CURSOR: &str = "agar.cursor";

// ── Camera zoom constants ─────────────────────────────────────────────────────

pub const BASE_ZOOM: f32 = 1.0;
const ZOOM_SCALE: f32 = 0.012;
const MIN_ZOOM: f32 = 0.15;
const ZOOM_LERP: f32 = 0.06;

const WORLD_SIZE: f32 = 2_000.0;
const GRID_SPACING: f32 = 100.0;

pub fn init_layers<Caps>(ctx: &ReducerContext<Caps>)
where
    Caps: CanInsert<ClientState>,
{
    // Layers above z=0: the graphics "default" layer (z=0, clear) clears the canvas
    // first, then our layers composite on top. Cursor at z=150 sits above the UI (z=100).
    let g = ctx.graphics();
    let _ = g.reducers.create_layer(LAYER_BG.to_string(), 1, false);
    let _ = g.reducers.create_layer(LAYER_WORLD.to_string(), 2, false);
    let _ = g.reducers.create_layer(LAYER_NAMES.to_string(), 3, false);
    let _ = g
        .reducers
        .create_layer(LAYER_CURSOR.to_string(), 150, false);

    // Initialise client state.
    let _ = ctx.current.tables.clientstate().insert(ClientState {
        id: 0,
        state: GameState::Lobby,
        zoom: BASE_ZOOM,
        target_zoom: BASE_ZOOM,
        final_score: 0.0,
    });
}

#[reducer(on = "graphics.frametick.update")]
pub fn on_frame<Caps>(ctx: ReducerContext<Caps>, _prev: FrameTick, _tick: FrameTick)
where
    Caps: CanRead<ClientState>
        + CanUpdate<ClientState>
        + CanRead<KeyState>
        + CanRead<MouseState>
        + CanRead<Player>
        + CanRead<Food>
        + CanRead<DeadPlayer>
        + CanRead<SurfaceInfo>
        + CanRead<UiElement>,
{
    let Some(mut cs) = ctx.current.tables.clientstate().get(0) else {
        return;
    };
    let g = ctx.graphics();
    let surface = g.tables.surfaceinfo().get(0);
    let (sw, sh) = match surface {
        Some(s) if s.width > 0 && s.height > 0 => (s.width as f32, s.height as f32),
        _ => return,
    };

    match cs.state {
        GameState::Lobby => {
            handle_lobby_input(&ctx, &mut cs, sw, sh);
        }
        GameState::InGame => {
            // Check for own death.
            let my_id = ctx.current_node_id();
            let dead = ctx
                .agar_server_example()
                .agar_server()
                .tables
                .deadplayer()
                .scan();
            if dead.iter().any(|d| d.id == my_id) {
                let my_radius = ctx
                    .agar_server_example()
                    .agar_server()
                    .tables
                    .player()
                    .get(my_id)
                    .map(|p| p.radius)
                    .unwrap_or(0.0);
                cs.final_score = my_radius;
                cs.state = GameState::Dead;
                let _ = ctx.ui().reducers.clear_focus();
                let _ = ctx.ui().reducers.clear_elements();
                show_dead_screen(&ctx, cs.final_score);
                let _ = ctx.current.tables.clientstate().update(cs);
                return;
            }
            handle_ingame_input(&ctx, sw, sh);
            render_world(&ctx, &mut cs, sw, sh);
            update_hud(&ctx, sw, sh);
        }
        GameState::Dead => {
            handle_dead_input(&ctx, &mut cs);
        }
    }

    // Draw a visible cursor on top of everything (above UI layer at z=100).
    let mouse = ctx.input().tables.mousestate().get(0);
    let (mx, my) = mouse.map(|m| m.position).unwrap_or((sw * 0.5, sh * 0.5));
    let g = ctx.graphics();
    let _ = g.reducers.draw_circle(
        LAYER_CURSOR.to_string(),
        Vec2 { x: mx, y: my },
        6.0,
        Color {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.9,
        },
        true,
        0.0,
    );
    let _ = g.reducers.draw_circle(
        LAYER_CURSOR.to_string(),
        Vec2 { x: mx, y: my },
        6.0,
        Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.6,
        },
        false,
        1.5,
    );

    let _ = ctx.current.tables.clientstate().update(cs);
}

fn render_world<Caps>(ctx: &ReducerContext<Caps>, cs: &mut ClientState, sw: f32, sh: f32)
where
    Caps: CanRead<Player> + CanRead<Food>,
{
    let server = ctx.agar_server_example().agar_server();
    let players = server.tables.player().scan();
    let foods = server.tables.food().scan();
    let my_id = ctx.current_node_id();

    let camera = players
        .iter()
        .find(|p| p.id == my_id)
        .map(|p| (p.pos.x, p.pos.y, p.radius))
        .unwrap_or((0.0, 0.0, 18.0));
    let (cam_x, cam_y, my_radius) = camera;

    // Smooth zoom towards target.
    cs.target_zoom = (BASE_ZOOM / (1.0 + my_radius * ZOOM_SCALE)).clamp(MIN_ZOOM, BASE_ZOOM);
    cs.zoom = cs.zoom + (cs.target_zoom - cs.zoom) * ZOOM_LERP;
    let zoom = cs.zoom;

    let g = ctx.graphics();

    // ── Background: dark fill + grid ─────────────────────────────────────────
    let bg_color = Color {
        r: 0.05,
        g: 0.05,
        b: 0.08,
        a: 1.0,
    };
    let _ = g.reducers.draw_rect(
        LAYER_BG.to_string(),
        Rect {
            x: 0.0,
            y: 0.0,
            w: sw,
            h: sh,
        },
        bg_color,
        true,
        0.0,
        None,
    );

    // World boundary.
    let bl = world_to_screen(-WORLD_SIZE, -WORLD_SIZE, cam_x, cam_y, zoom, sw, sh);
    let br = world_to_screen(WORLD_SIZE, WORLD_SIZE, cam_x, cam_y, zoom, sw, sh);
    let boundary_w = br.0 - bl.0;
    let boundary_h = br.1 - bl.1;
    let _ = g.reducers.draw_rect(
        LAYER_BG.to_string(),
        Rect {
            x: bl.0,
            y: bl.1,
            w: boundary_w,
            h: boundary_h,
        },
        Color {
            r: 0.10,
            g: 0.10,
            b: 0.14,
            a: 1.0,
        },
        true,
        0.0,
        None,
    );
    let _ = g.reducers.draw_rect(
        LAYER_BG.to_string(),
        Rect {
            x: bl.0,
            y: bl.1,
            w: boundary_w,
            h: boundary_h,
        },
        Color {
            r: 0.30,
            g: 0.30,
            b: 0.40,
            a: 1.0,
        },
        false,
        2.0,
        None,
    );

    // Grid lines (only draw visible portion).

    let start_x = snap_to_grid(cam_x - sw / (2.0 * zoom), GRID_SPACING);
    let start_y = snap_to_grid(cam_y - sh / (2.0 * zoom), GRID_SPACING);
    let end_x = cam_x + sw / (2.0 * zoom) + GRID_SPACING;
    let end_y = cam_y + sh / (2.0 * zoom) + GRID_SPACING;

    let mut gx = start_x;
    while gx <= end_x {
        let sx = world_to_screen_x(gx, cam_x, zoom, sw);
        let _ = g.reducers.draw_polyline(
            LAYER_BG.to_string(),
            vec![Vec2 { x: sx, y: 0.0 }, Vec2 { x: sx, y: sh }],
            1,
            Color {
                r: 0.18,
                g: 0.18,
                b: 0.24,
                a: 1.0,
            },
            false,
            false,
        );
        gx += GRID_SPACING;
    }
    let mut gy = start_y;
    while gy <= end_y {
        let sy = world_to_screen_y(gy, cam_y, zoom, sh);
        let _ = g.reducers.draw_polyline(
            LAYER_BG.to_string(),
            vec![Vec2 { x: 0.0, y: sy }, Vec2 { x: sw, y: sy }],
            1,
            Color {
                r: 0.18,
                g: 0.18,
                b: 0.24,
                a: 1.0,
            },
            false,
            false,
        );
        gy += GRID_SPACING;
    }

    // ── Food ─────────────────────────────────────────────────────────────────
    // Group food by color for batch draws.
    let mut batches: Vec<(Color, Vec<Vec2>, Vec<f32>)> = Vec::new();
    for food in &foods {
        let color = Color {
            r: food.color.r,
            g: food.color.g,
            b: food.color.b,
            a: 1.0,
        };
        let pos = world_to_screen(food.pos.x, food.pos.y, cam_x, cam_y, zoom, sw, sh);
        let radius = food.radius * zoom;
        if let Some(batch) = batches.iter_mut().find(|(c, _, _)| colors_eq(c, &color)) {
            batch.1.push(Vec2 { x: pos.0, y: pos.1 });
            batch.2.push(radius);
        } else {
            batches.push((color, vec![Vec2 { x: pos.0, y: pos.1 }], vec![radius]));
        }
    }
    for (color, centers, radii) in batches {
        let _ = g
            .reducers
            .draw_circles(LAYER_WORLD.to_string(), centers, radii, color, true, 0.0);
    }

    // ── Players ───────────────────────────────────────────────────────────────
    for player in &players {
        let pos = world_to_screen(player.pos.x, player.pos.y, cam_x, cam_y, zoom, sw, sh);
        let radius = player.radius * zoom;
        let pr = player.color.r;
        let pg = player.color.g;
        let pb = player.color.b;
        let is_me = player.id == my_id;

        // Player circle.
        let _ = g.reducers.draw_circle(
            LAYER_WORLD.to_string(),
            Vec2 { x: pos.0, y: pos.1 },
            radius,
            Color {
                r: pr,
                g: pg,
                b: pb,
                a: 1.0,
            },
            true,
            0.0,
        );
        // Outline (slightly darker, thicker for self).
        let outline_w = if is_me { 3.0 } else { 1.5 };
        let _ = g.reducers.draw_circle(
            LAYER_WORLD.to_string(),
            Vec2 { x: pos.0, y: pos.1 },
            radius,
            Color {
                r: pr * 0.6,
                g: pg * 0.6,
                b: pb * 0.6,
                a: 1.0,
            },
            false,
            outline_w,
        );

        // Name centered above circle.
        let name_chars = player.name.chars().count() as f32;
        let name_size = (12.0 * zoom).clamp(10.0, 18.0);
        let advance = 9.0 * (name_size / 8.0).max(0.125);
        let name_x = pos.0 - name_chars * advance * 0.5;
        let name_y = pos.1 - radius - name_size * 1.5;
        let _ = g.reducers.draw_text(
            LAYER_NAMES.to_string(),
            player.name.clone(),
            Vec2 {
                x: name_x,
                y: name_y,
            },
            name_size,
            Color {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.9,
            },
            None,
        );

        // Score below name.
        let score_str = format!("{}", player.radius as u32);
        let score_chars = score_str.chars().count() as f32;
        let score_x = pos.0 - score_chars * advance * 0.5;
        let _ = g.reducers.draw_text(
            LAYER_NAMES.to_string(),
            score_str,
            Vec2 {
                x: score_x,
                y: name_y + name_size,
            },
            name_size * 0.85,
            Color {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.65,
            },
            None,
        );
    }
}

fn world_to_screen(
    wx: f32,
    wy: f32,
    cam_x: f32,
    cam_y: f32,
    zoom: f32,
    sw: f32,
    sh: f32,
) -> (f32, f32) {
    (
        (wx - cam_x) * zoom + sw * 0.5,
        (wy - cam_y) * zoom + sh * 0.5,
    )
}

fn world_to_screen_x(wx: f32, cam_x: f32, zoom: f32, sw: f32) -> f32 {
    (wx - cam_x) * zoom + sw * 0.5
}

fn world_to_screen_y(wy: f32, cam_y: f32, zoom: f32, sh: f32) -> f32 {
    (wy - cam_y) * zoom + sh * 0.5
}

fn snap_to_grid(v: f32, grid: f32) -> f32 {
    (v / grid).floor() * grid
}

fn colors_eq(a: &Color, b: &Color) -> bool {
    (a.r - b.r).abs() < 0.001 && (a.g - b.g).abs() < 0.001 && (a.b - b.b).abs() < 0.001
}
