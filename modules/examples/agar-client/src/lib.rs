use crate::bindings::{
    agar_server::{
        HasAgarServerModuleHandle,
        agar_server::{
            DeadPlayer, Food, HasDeadPlayerHandle, HasFoodHandle, HasPlayerHandle,
            Player,
        },
    },
    graphics::*,
    input::*,
    ui::*,
    *,
};
use interstice_sdk::{key_code::KeyCode, *};

interstice_module!(
    visibility: Public,
    replicated_tables: [
        "agar-server.agar-server.player",
        "agar-server.agar-server.food",
        "agar-server.agar-server.deadplayer",
    ]
);

// ── Layers ────────────────────────────────────────────────────────────────────

const LAYER_BG: &str = "agar.bg";
const LAYER_WORLD: &str = "agar.world";
const LAYER_NAMES: &str = "agar.names";
const LAYER_CURSOR: &str = "agar.cursor";

// ── Camera zoom constants ─────────────────────────────────────────────────────

const BASE_ZOOM: f32 = 1.0;
const ZOOM_SCALE: f32 = 0.012;
const MIN_ZOOM: f32 = 0.15;
const ZOOM_LERP: f32 = 0.06;

const WORLD_SIZE: f32 = 2_000.0;
const GRID_SPACING: f32 = 100.0;

// ── Game state ────────────────────────────────────────────────────────────────

#[interstice_type]
#[derive(Debug, PartialEq)]
pub enum GameState {
    Lobby,
    InGame,
    Dead,
}

#[table(ephemeral)]
pub struct ClientState {
    #[primary_key]
    id: u32,
    state: GameState,
    zoom: f32,
    target_zoom: f32,
    final_score: f32,
}

// ── UI element IDs ────────────────────────────────────────────────────────────

const UI_LOBBY_ROOT: &str = "lobby_root";
const UI_LOBBY_CARD: &str = "lobby_card";
const UI_LOBBY_TITLE: &str = "lobby_title";
const UI_LOBBY_INPUT: &str = "lobby_name_input";
const UI_LOBBY_BTN: &str = "lobby_play_btn";

const UI_HUD_SCORE: &str = "hud_score";
const UI_HUD_LB_ROOT: &str = "hud_lb_root";
const UI_HUD_LB_TITLE: &str = "hud_lb_title";

const UI_DEAD_ROOT: &str = "dead_root";
const UI_DEAD_CARD: &str = "dead_card";
const UI_DEAD_MSG: &str = "dead_msg";
const UI_DEAD_SCORE: &str = "dead_score";
const UI_DEAD_BTN: &str = "dead_play_btn";

// ── Load ──────────────────────────────────────────────────────────────────────

#[reducer(on = "load")]
pub fn init<Caps>(ctx: ReducerContext<Caps>)
where
    Caps: CanInsert<ClientState>,
{
    // Layers above z=0: the graphics "default" layer (z=0, clear) clears the canvas
    // first, then our layers composite on top. Cursor at z=150 sits above the UI (z=100).
    let g = ctx.graphics();
    let _ = g.reducers.create_layer(LAYER_BG.to_string(), 1, false);
    let _ = g.reducers.create_layer(LAYER_WORLD.to_string(), 2, false);
    let _ = g.reducers.create_layer(LAYER_NAMES.to_string(), 3, false);
    let _ = g.reducers.create_layer(LAYER_CURSOR.to_string(), 150, false);

    // Initialise client state.
    let _ = ctx.current.tables.clientstate().insert(ClientState {
        id: 0,
        state: GameState::Lobby,
        zoom: BASE_ZOOM,
        target_zoom: BASE_ZOOM,
        final_score: 0.0,
    });

    build_lobby_ui(&ctx);
}

// ── Frame tick ────────────────────────────────────────────────────────────────

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
    let Some(mut cs) = ctx.current.tables.clientstate().get(0) else { return };
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
            let dead = ctx.agar_server().agar_server().tables.deadplayer().scan();
            if dead.iter().any(|d| d.id == my_id) {
                let my_radius = ctx.agar_server().agar_server().tables.player()
                    .get(my_id).map(|p| p.radius).unwrap_or(0.0);
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
        Color { r: 1.0, g: 1.0, b: 1.0, a: 0.9 },
        true, 0.0,
    );
    let _ = g.reducers.draw_circle(
        LAYER_CURSOR.to_string(),
        Vec2 { x: mx, y: my },
        6.0,
        Color { r: 0.0, g: 0.0, b: 0.0, a: 0.6 },
        false, 1.5,
    );

    let _ = ctx.current.tables.clientstate().update(cs);
}

// ── Lobby ─────────────────────────────────────────────────────────────────────

fn build_lobby_ui<Caps>(ctx: &ReducerContext<Caps>) {
    let ui = ctx.ui();
    let none = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let text_col = (0.92f32, 0.92f32, 0.95f32, 1.0f32);
    let muted = (0.55f32, 0.55f32, 0.62f32, 1.0f32);
    let card_bg = (0.14f32, 0.14f32, 0.18f32, 1.0f32);
    let input_bg = (0.10f32, 0.10f32, 0.13f32, 1.0f32);
    let btn_bg = (0.27f32, 0.47f32, 0.87f32, 1.0f32);

    let spacer = |id: &str, parent: &str, order: u32| {
        let _ = ui.reducers.create_element(UiElement {
            id: id.into(), parent: Some(parent.into()), order,
            width: Size::Grow, height: Size::Grow,
            layout_direction: LayoutDirection::Row,
            gap: 0.0, padding: 0.0, margin: 0.0,
            background_color: none, corner_radius: 0.0,
            border_width: 0.0, border_color: none,
            text: None, text_size: 0.0, text_color: none,
            text_wrap: TextWrap::None, is_input: false, cursor_pos: 0,
            scrollable_x: false, scrollable_y: false, scroll_x: 0.0, scroll_y: 0.0,
            visible: true,
        });
    };

    // Full-screen dark root (Column — vertical centering via spacers).
    let _ = ui.reducers.create_element(UiElement {
        id: UI_LOBBY_ROOT.into(), parent: None, order: 0,
        width: Size::Grow, height: Size::Grow, layout_direction: LayoutDirection::Column,
        gap: 0.0, padding: 0.0, margin: 0.0,
        background_color: (0.07, 0.07, 0.10, 1.0), corner_radius: 0.0,
        border_width: 0.0, border_color: none,
        text: None, text_size: 0.0, text_color: none,
        text_wrap: TextWrap::None, is_input: false, cursor_pos: 0,
        scrollable_x: false, scrollable_y: false, scroll_x: 0.0, scroll_y: 0.0,
        visible: true,
    });

    // Top spacer — pushes center row down.
    spacer("lobby_v_top", UI_LOBBY_ROOT, 0);

    // Center row — horizontal centering via spacers.
    let _ = ui.reducers.create_element(UiElement {
        id: "lobby_center_row".into(), parent: Some(UI_LOBBY_ROOT.into()), order: 1,
        width: Size::Grow, height: Size::Fit, layout_direction: LayoutDirection::Row,
        gap: 0.0, padding: 0.0, margin: 0.0,
        background_color: none, corner_radius: 0.0,
        border_width: 0.0, border_color: none,
        text: None, text_size: 0.0, text_color: none,
        text_wrap: TextWrap::None, is_input: false, cursor_pos: 0,
        scrollable_x: false, scrollable_y: false, scroll_x: 0.0, scroll_y: 0.0,
        visible: true,
    });

    // Left spacer — pushes card right.
    spacer("lobby_h_left", "lobby_center_row", 0);

    // Card itself.
    let _ = ui.reducers.create_element(UiElement {
        id: UI_LOBBY_CARD.into(), parent: Some("lobby_center_row".into()), order: 1,
        width: Size::Fixed(360.0), height: Size::Fit, layout_direction: LayoutDirection::Column,
        gap: 16.0, padding: 28.0, margin: 0.0,
        background_color: card_bg, corner_radius: 12.0,
        border_width: 1.0, border_color: (0.28, 0.28, 0.34, 1.0),
        text: None, text_size: 0.0, text_color: none,
        text_wrap: TextWrap::None, is_input: false, cursor_pos: 0,
        scrollable_x: false, scrollable_y: false, scroll_x: 0.0, scroll_y: 0.0,
        visible: true,
    });

    // Right spacer — pushes card left.
    spacer("lobby_h_right", "lobby_center_row", 2);

    // Bottom spacer — pushes center row up.
    spacer("lobby_v_bot", UI_LOBBY_ROOT, 2);

    // Card contents.
    let _ = ui.reducers.create_element(UiElement {
        id: UI_LOBBY_TITLE.into(), parent: Some(UI_LOBBY_CARD.into()), order: 0,
        width: Size::Grow, height: Size::Fit, layout_direction: LayoutDirection::Row,
        gap: 0.0, padding: 0.0, margin: 0.0,
        background_color: none, corner_radius: 0.0,
        border_width: 0.0, border_color: none,
        text: Some("agar.io".into()), text_size: 28.0, text_color: text_col,
        text_wrap: TextWrap::None, is_input: false, cursor_pos: 0,
        scrollable_x: false, scrollable_y: false, scroll_x: 0.0, scroll_y: 0.0,
        visible: true,
    });
    let _ = ui.reducers.create_element(UiElement {
        id: "lobby_sub".into(), parent: Some(UI_LOBBY_CARD.into()), order: 1,
        width: Size::Grow, height: Size::Fit, layout_direction: LayoutDirection::Row,
        gap: 0.0, padding: 0.0, margin: 0.0,
        background_color: none, corner_radius: 0.0,
        border_width: 0.0, border_color: none,
        text: Some("Enter your name to play".into()), text_size: 13.0, text_color: muted,
        text_wrap: TextWrap::None, is_input: false, cursor_pos: 0,
        scrollable_x: false, scrollable_y: false, scroll_x: 0.0, scroll_y: 0.0,
        visible: true,
    });
    let _ = ui.reducers.create_element(UiElement {
        id: UI_LOBBY_INPUT.into(), parent: Some(UI_LOBBY_CARD.into()), order: 2,
        width: Size::Grow, height: Size::Fixed(36.0), layout_direction: LayoutDirection::Row,
        gap: 0.0, padding: 8.0, margin: 0.0,
        background_color: input_bg, corner_radius: 6.0,
        border_width: 1.0, border_color: (0.28, 0.28, 0.34, 1.0),
        text: Some(String::new()), text_size: 14.0, text_color: text_col,
        text_wrap: TextWrap::None, is_input: true, cursor_pos: 0,
        scrollable_x: false, scrollable_y: false, scroll_x: 0.0, scroll_y: 0.0,
        visible: true,
    });
    let _ = ui.reducers.create_element(UiElement {
        id: UI_LOBBY_BTN.into(), parent: Some(UI_LOBBY_CARD.into()), order: 3,
        width: Size::Grow, height: Size::Fixed(40.0), layout_direction: LayoutDirection::Row,
        gap: 0.0, padding: 8.0, margin: 0.0,
        background_color: btn_bg, corner_radius: 8.0,
        border_width: 0.0, border_color: none,
        text: Some("Play".into()), text_size: 16.0, text_color: (1.0, 1.0, 1.0, 1.0),
        text_wrap: TextWrap::None, is_input: false, cursor_pos: 0,
        scrollable_x: false, scrollable_y: false, scroll_x: 0.0, scroll_y: 0.0,
        visible: true,
    });

    let _ = ui.reducers.set_focus(UI_LOBBY_INPUT.into());
}

fn handle_lobby_input<Caps>(ctx: &ReducerContext<Caps>, cs: &mut ClientState, sw: f32, sh: f32)
where
    Caps: CanRead<ClientState> + CanUpdate<ClientState> + CanRead<KeyState> + CanRead<UiElement>,
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
    Caps: CanRead<ClientState> + CanUpdate<ClientState> + CanRead<UiElement>,
{
    // Read the typed name directly from the UI input element (managed by the UI module).
    let name = ctx.ui().tables.uielement()
        .get(UI_LOBBY_INPUT.to_string())
        .and_then(|el| el.text)
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "Player".to_string());

    let server = ctx.agar_server().agar_server();
    if let Err(err) = server.reducers.join(name) {
        ctx.log(&format!("join failed: {err}"));
        return;
    }

    // Clear focus BEFORE clearing elements so the UI module doesn't try to
    // update the focused (now-deleted) lobby input element on subsequent frames.
    let _ = ctx.ui().reducers.clear_focus();
    let _ = ctx.ui().reducers.clear_elements();
    build_hud_ui(ctx);

    cs.state = GameState::InGame;
    cs.zoom = BASE_ZOOM;
    cs.target_zoom = BASE_ZOOM;
}

// ── In-game input + rendering ─────────────────────────────────────────────────

fn handle_ingame_input<Caps>(ctx: &ReducerContext<Caps>, sw: f32, sh: f32)
where
    Caps: CanRead<KeyState> + CanRead<MouseState>,
{
    let (dx, dy) = steering_dir(ctx, sw, sh);
    let server = ctx.agar_server().agar_server();
    let _ = server.reducers.set_direction(dx, dy);
}

fn steering_dir<Caps>(ctx: &ReducerContext<Caps>, sw: f32, sh: f32) -> (f32, f32)
where
    Caps: CanRead<KeyState> + CanRead<MouseState>,
{
    // Mouse direction: from accumulated position relative to screen center.
    let mouse = ctx.input().tables.mousestate().get(0);
    let (mx, my) = mouse.map(|m| m.position).unwrap_or((0.0, 0.0));
    let mut dx = mx - sw * 0.5;
    let mut dy = my - sh * 0.5;

    // Keyboard overlay (additive).
    let keys = ctx.input().tables.keystate().scan();
    let pressed = |code: u32| keys.iter().any(|k| k.code == code && k.pressed);
    if pressed(KeyCode::KeyA as u32) || pressed(KeyCode::ArrowLeft as u32)  { dx -= sw * 0.5; }
    if pressed(KeyCode::KeyD as u32) || pressed(KeyCode::ArrowRight as u32) { dx += sw * 0.5; }
    if pressed(KeyCode::KeyW as u32) || pressed(KeyCode::ArrowUp as u32)    { dy -= sh * 0.5; }
    if pressed(KeyCode::KeyS as u32) || pressed(KeyCode::ArrowDown as u32)  { dy += sh * 0.5; }

    let len = (dx * dx + dy * dy).sqrt();
    if len > 0.001 { (dx / len, dy / len) } else { (0.0, 0.0) }
}

fn render_world<Caps>(ctx: &ReducerContext<Caps>, cs: &mut ClientState, sw: f32, sh: f32)
where
    Caps: CanRead<Player> + CanRead<Food>,
{
    let server = ctx.agar_server().agar_server();
    let players = server.tables.player().scan();
    let foods = server.tables.food().scan();
    let my_id = ctx.current_node_id();

    let camera = players.iter()
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
    let bg_color = Color { r: 0.05, g: 0.05, b: 0.08, a: 1.0 };
    let _ = g.reducers.draw_rect(
        LAYER_BG.to_string(),
        Rect { x: 0.0, y: 0.0, w: sw, h: sh },
        bg_color, true, 0.0, None,
    );

    // World boundary.
    let bl = world_to_screen(-WORLD_SIZE, -WORLD_SIZE, cam_x, cam_y, zoom, sw, sh);
    let br = world_to_screen( WORLD_SIZE,  WORLD_SIZE, cam_x, cam_y, zoom, sw, sh);
    let boundary_w = br.0 - bl.0;
    let boundary_h = br.1 - bl.1;
    let _ = g.reducers.draw_rect(
        LAYER_BG.to_string(),
        Rect { x: bl.0, y: bl.1, w: boundary_w, h: boundary_h },
        Color { r: 0.10, g: 0.10, b: 0.14, a: 1.0 },
        true, 0.0, None,
    );
    let _ = g.reducers.draw_rect(
        LAYER_BG.to_string(),
        Rect { x: bl.0, y: bl.1, w: boundary_w, h: boundary_h },
        Color { r: 0.30, g: 0.30, b: 0.40, a: 1.0 },
        false, 2.0, None,
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
            1, Color { r: 0.18, g: 0.18, b: 0.24, a: 1.0 }, false, false,
        );
        gx += GRID_SPACING;
    }
    let mut gy = start_y;
    while gy <= end_y {
        let sy = world_to_screen_y(gy, cam_y, zoom, sh);
        let _ = g.reducers.draw_polyline(
            LAYER_BG.to_string(),
            vec![Vec2 { x: 0.0, y: sy }, Vec2 { x: sw, y: sy }],
            1, Color { r: 0.18, g: 0.18, b: 0.24, a: 1.0 }, false, false,
        );
        gy += GRID_SPACING;
    }

    // ── Food ─────────────────────────────────────────────────────────────────
    // Group food by color for batch draws.
    let mut batches: Vec<(Color, Vec<Vec2>, Vec<f32>)> = Vec::new();
    for food in &foods {
        let color = Color { r: food.color.r, g: food.color.g, b: food.color.b, a: 1.0 };
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
        let _ = g.reducers.draw_circles(LAYER_WORLD.to_string(), centers, radii, color, true, 0.0);
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
            Color { r: pr, g: pg, b: pb, a: 1.0 },
            true, 0.0,
        );
        // Outline (slightly darker, thicker for self).
        let outline_w = if is_me { 3.0 } else { 1.5 };
        let _ = g.reducers.draw_circle(
            LAYER_WORLD.to_string(),
            Vec2 { x: pos.0, y: pos.1 },
            radius,
            Color { r: pr * 0.6, g: pg * 0.6, b: pb * 0.6, a: 1.0 },
            false, outline_w,
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
            Vec2 { x: name_x, y: name_y },
            name_size,
            Color { r: 1.0, g: 1.0, b: 1.0, a: 0.9 },
            None,
        );

        // Score below name.
        let score_str = format!("{}", player.radius as u32);
        let score_chars = score_str.chars().count() as f32;
        let score_x = pos.0 - score_chars * advance * 0.5;
        let _ = g.reducers.draw_text(
            LAYER_NAMES.to_string(),
            score_str,
            Vec2 { x: score_x, y: name_y + name_size },
            name_size * 0.85,
            Color { r: 1.0, g: 1.0, b: 1.0, a: 0.65 },
            None,
        );
    }
}

// ── HUD ───────────────────────────────────────────────────────────────────────

fn build_hud_ui<Caps>(ctx: &ReducerContext<Caps>) {
    let ui = ctx.ui();
    let none = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let text_col = (0.92f32, 0.92f32, 0.95f32, 1.0f32);
    let panel_bg = (0.07f32, 0.07f32, 0.10f32, 0.80f32);

    // Single root row spanning the full width at the top.
    let _ = ui.reducers.create_element(UiElement {
        id: "hud_root".into(), parent: None, order: 10,
        width: Size::Grow, height: Size::Fit,
        layout_direction: LayoutDirection::Row,
        gap: 0.0, padding: 12.0, margin: 0.0,
        background_color: none, corner_radius: 0.0,
        border_width: 0.0, border_color: none,
        text: None, text_size: 0.0, text_color: none,
        text_wrap: TextWrap::None, is_input: false, cursor_pos: 0,
        scrollable_x: false, scrollable_y: false, scroll_x: 0.0, scroll_y: 0.0,
        visible: true,
    });

    // Score label (top-left).
    let _ = ui.reducers.create_element(UiElement {
        id: UI_HUD_SCORE.into(), parent: Some("hud_root".into()), order: 0,
        width: Size::Fit, height: Size::Fit,
        layout_direction: LayoutDirection::Row,
        gap: 0.0, padding: 8.0, margin: 0.0,
        background_color: panel_bg, corner_radius: 6.0,
        border_width: 0.0, border_color: none,
        text: Some("Score: 0".into()), text_size: 14.0, text_color: text_col,
        text_wrap: TextWrap::None, is_input: false, cursor_pos: 0,
        scrollable_x: false, scrollable_y: false, scroll_x: 0.0, scroll_y: 0.0,
        visible: true,
    });

    // Middle spacer — pushes leaderboard to the right.
    let _ = ui.reducers.create_element(UiElement {
        id: "hud_mid_spacer".into(), parent: Some("hud_root".into()), order: 1,
        width: Size::Grow, height: Size::Grow,
        layout_direction: LayoutDirection::Row,
        gap: 0.0, padding: 0.0, margin: 0.0,
        background_color: none, corner_radius: 0.0,
        border_width: 0.0, border_color: none,
        text: None, text_size: 0.0, text_color: none,
        text_wrap: TextWrap::None, is_input: false, cursor_pos: 0,
        scrollable_x: false, scrollable_y: false, scroll_x: 0.0, scroll_y: 0.0,
        visible: true,
    });

    // Leaderboard panel (top-right).
    let _ = ui.reducers.create_element(UiElement {
        id: UI_HUD_LB_ROOT.into(), parent: Some("hud_root".into()), order: 2,
        width: Size::Fixed(160.0), height: Size::Fit,
        layout_direction: LayoutDirection::Column,
        gap: 4.0, padding: 10.0, margin: 0.0,
        background_color: panel_bg, corner_radius: 8.0,
        border_width: 0.0, border_color: none,
        text: None, text_size: 0.0, text_color: none,
        text_wrap: TextWrap::None, is_input: false, cursor_pos: 0,
        scrollable_x: false, scrollable_y: false, scroll_x: 0.0, scroll_y: 0.0,
        visible: true,
    });
    let _ = ui.reducers.create_element(UiElement {
        id: UI_HUD_LB_TITLE.into(), parent: Some(UI_HUD_LB_ROOT.into()), order: 0,
        width: Size::Grow, height: Size::Fit,
        layout_direction: LayoutDirection::Row,
        gap: 0.0, padding: 0.0, margin: 0.0,
        background_color: none, corner_radius: 0.0,
        border_width: 0.0, border_color: none,
        text: Some("Leaderboard".into()), text_size: 13.0, text_color: (0.7, 0.7, 0.8, 1.0),
        text_wrap: TextWrap::None, is_input: false, cursor_pos: 0,
        scrollable_x: false, scrollable_y: false, scroll_x: 0.0, scroll_y: 0.0,
        visible: true,
    });
}

fn update_hud<Caps>(ctx: &ReducerContext<Caps>, _sw: f32, _sh: f32)
where
    Caps: CanRead<Player>,
{
    let my_id = ctx.current_node_id();
    let mut players: Vec<Player> = ctx.agar_server().agar_server().tables.player().scan();
    players.sort_by(|a, b| b.radius.partial_cmp(&a.radius).unwrap_or(std::cmp::Ordering::Equal));

    let ui = ctx.ui();
    let none = (0.0f32, 0.0f32, 0.0f32, 0.0f32);

    // Update score label.
    if let Some(my_player) = players.iter().find(|p| p.id == my_id) {
        let _ = ui.reducers.update_element(UiElement {
            id: UI_HUD_SCORE.into(), parent: Some("hud_root".into()), order: 0,
            width: Size::Fit, height: Size::Fit,
            layout_direction: LayoutDirection::Row,
            gap: 0.0, padding: 8.0, margin: 0.0,
            background_color: (0.07, 0.07, 0.10, 0.80),
            corner_radius: 6.0, border_width: 0.0, border_color: none,
            text: Some(format!("Score: {}", my_player.radius as u32)),
            text_size: 14.0, text_color: (0.92, 0.92, 0.95, 1.0),
            text_wrap: TextWrap::None, is_input: false, cursor_pos: 0,
            scrollable_x: false, scrollable_y: false, scroll_x: 0.0, scroll_y: 0.0,
            visible: true,
        });
    }

    // Rebuild leaderboard rows (top 8).
    // Remove old rows.
    for i in 0..8 {
        let _ = ui.reducers.delete_element(format!("hud_lb_{i}"));
    }
    let text_col = (0.92f32, 0.92f32, 0.95f32, 1.0f32);
    for (i, p) in players.iter().take(8).enumerate() {
        let is_me = p.id == my_id;
        let name_color = if is_me { (p.color.r, p.color.g, p.color.b, 1.0) } else { text_col };
        let label = format!("{}. {} ({})", i + 1, p.name, p.radius as u32);
        let _ = ui.reducers.create_element(UiElement {
            id: format!("hud_lb_{i}"),
            parent: Some(UI_HUD_LB_ROOT.into()),
            order: (i + 1) as u32,
            width: Size::Grow, height: Size::Fit,
            layout_direction: LayoutDirection::Row,
            gap: 0.0, padding: 2.0, margin: 0.0,
            background_color: none, corner_radius: 0.0,
            border_width: 0.0, border_color: none,
            text: Some(label), text_size: 11.0, text_color: name_color,
            text_wrap: TextWrap::None, is_input: false, cursor_pos: 0,
            scrollable_x: false, scrollable_y: false, scroll_x: 0.0, scroll_y: 0.0,
            visible: true,
        });
    }
}

// ── Death screen ──────────────────────────────────────────────────────────────

fn show_dead_screen<Caps>(ctx: &ReducerContext<Caps>, final_score: f32) {
    let ui = ctx.ui();
    let none = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let text_col = (0.92f32, 0.92f32, 0.95f32, 1.0f32);
    let card_bg = (0.14f32, 0.14f32, 0.18f32, 1.0f32);
    let btn_bg = (0.27f32, 0.47f32, 0.87f32, 1.0f32);

    let mk = |id: &str, parent: Option<&str>, order: u32,
               w: Size, h: Size,
               gap: f32, pad: f32, mar: f32,
               bg: (f32,f32,f32,f32), cr: f32,
               text: Option<String>, ts: f32, tc: (f32,f32,f32,f32)| {
        let _ = ui.reducers.create_element(UiElement {
            id: id.into(), parent: parent.map(|s| s.into()), order,
            width: w, height: h,
            layout_direction: LayoutDirection::Column,
            gap, padding: pad, margin: mar,
            background_color: bg, corner_radius: cr,
            border_width: 0.0, border_color: none,
            text, text_size: ts, text_color: tc,
            text_wrap: TextWrap::None, is_input: false, cursor_pos: 0,
            scrollable_x: false, scrollable_y: false, scroll_x: 0.0, scroll_y: 0.0,
            visible: true,
        });
    };

    mk(UI_DEAD_ROOT, None, 20, Size::Grow, Size::Grow, 0.0, 0.0, 0.0,
       (0.0, 0.0, 0.0, 0.6), 0.0, None, 0.0, none);

    mk(UI_DEAD_CARD, Some(UI_DEAD_ROOT), 0, Size::Fixed(300.0), Size::Fit,
       20.0, 32.0, 0.0, card_bg, 12.0, None, 0.0, none);

    mk(UI_DEAD_MSG, Some(UI_DEAD_CARD), 0, Size::Grow, Size::Fit,
       0.0, 0.0, 0.0, none, 0.0,
       Some("You were eaten!".into()), 22.0, (0.95, 0.4, 0.4, 1.0));

    mk(UI_DEAD_SCORE, Some(UI_DEAD_CARD), 1, Size::Grow, Size::Fit,
       0.0, 0.0, 0.0, none, 0.0,
       Some(format!("Final score: {}", final_score as u32)), 14.0, text_col);

    mk(UI_DEAD_BTN, Some(UI_DEAD_CARD), 2, Size::Grow, Size::Fixed(40.0),
       0.0, 8.0, 0.0, btn_bg, 8.0,
       Some("Play again".into()), 16.0, (1.0, 1.0, 1.0, 1.0));
}

fn handle_dead_input<Caps>(ctx: &ReducerContext<Caps>, cs: &mut ClientState)
where
    Caps: CanRead<KeyState> + CanRead<ClientState>,
{
    let key_states = ctx.input().tables.keystate().scan();
    let enter = key_states.iter().any(|k| {
        k.pressed && (k.code == KeyCode::Enter as u32 || k.code == KeyCode::NumpadEnter as u32)
    });
    if enter {
        let _ = ctx.ui().reducers.clear_focus();
        let _ = ctx.ui().reducers.clear_elements();
        build_lobby_ui(ctx);
        cs.state = GameState::Lobby;
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn world_to_screen(wx: f32, wy: f32, cam_x: f32, cam_y: f32, zoom: f32, sw: f32, sh: f32) -> (f32, f32) {
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
    (a.r - b.r).abs() < 0.001
        && (a.g - b.g).abs() < 0.001
        && (a.b - b.b).abs() < 0.001
}

