use crate::bindings::{
    agar_server::HasAgarServerModuleHandle,
    agar_server::agar_server::{Snapshot, Vec2 as AgarVec2},
    graphics::*,
    input::*,
    *,
};
use interstice_sdk::*;

interstice_module!(visibility: Public);

const LAYER: &str = "agar.world";
const CAMERA_W: f32 = 1280.0;
const CAMERA_H: f32 = 720.0;

#[reducer(on = "load")]
pub fn init(ctx: ReducerContext) {
    let graphics = ctx.graphics();
    let _ = graphics.reducers.create_layer(LAYER.to_string(), 0, true);
    let _ = graphics.reducers.set_layer_clear(LAYER.to_string(), true);

    let server = ctx.agar_server().agar_server();
    if let Err(err) = server.reducers.join("TestPlayer".to_string()) {
        ctx.log(&format!("join failed: {}", err));
    }
}

#[reducer(on = "graphics.frametick.update")]
pub fn on_frame(ctx: ReducerContext, _prev: FrameTick, _tick: FrameTick) {
    let server = ctx.agar_server().agar_server();

    let (dx, dy) = input_dir(&ctx);
    if let Err(err) = server.reducers.set_direction(dx, dy) {
        ctx.log(&format!("set_direction failed: {}", err));
    }

    let snapshot = match server.queries.snapshot() {
        Ok(s) => s,
        Err(err) => {
            ctx.log(&format!("snapshot failed: {}", err));
            return;
        }
    };

    render_world(&ctx, &snapshot);
}

fn input_dir(ctx: &ReducerContext) -> (f32, f32) {
    let input = ctx.input();

    // Helper to check if a key is pressed
    let pressed = |code: u32| {
        input
            .tables
            .keystate()
            .scan()
            .unwrap_or_default()
            .iter()
            .any(|k| k.code == code && k.pressed)
    };

    let mut dx = 0.0;
    let mut dy = 0.0;
    if pressed(4) || pressed(80) {
        dx -= 1.0; // A or Left
    }
    if pressed(7) || pressed(79) {
        dx += 1.0; // D or Right
    }
    if pressed(26) || pressed(82) {
        dy -= 1.0; // W or Up
    }
    if pressed(22) || pressed(81) {
        dy += 1.0; // S or Down
    }

    // Mouse motion accumulates; use it as a gentle nudge toward cursor position.
    let mouse_opt = input
        .tables
        .mousestate()
        .scan()
        .unwrap_or_default()
        .into_iter()
        .find(|m| m.id == 0);
    if let Some(MouseState { position, .. }) = mouse_opt {
        if position.0.abs() > 1.0 || position.1.abs() > 1.0 {
            dx += position.0.signum() * 0.3;
            dy += position.1.signum() * 0.3;
        }
    }

    let len = (dx * dx + dy * dy).sqrt();
    if len > 0.0001 {
        (dx / len, dy / len)
    } else {
        (0.0, 0.0)
    }
}

fn render_world(ctx: &ReducerContext, snapshot: &Snapshot) {
    let graphics = ctx.graphics();
    let camera = snapshot
        .players
        .iter()
        .find(|p| p.id == ctx.current_node_id())
        .map(|p| &p.pos)
        .unwrap_or(&AgarVec2 { x: 0.0, y: 0.0 });

    for food in &snapshot.foods {
        let pos = world_to_screen(&food.pos, camera);
        let _ = graphics.reducers.draw_circle(
            LAYER.to_string(),
            pos,
            food.radius,
            Color {
                r: 0.3,
                g: 0.85,
                b: 0.45,
                a: 1.0,
            },
            true,
            0.0,
        );
    }

    for player in &snapshot.players {
        let pos = world_to_screen(&player.pos, camera);
        let color = color_from_id(&player.id);
        let _ = graphics.reducers.draw_circle(
            LAYER.to_string(),
            Vec2 { x: pos.x, y: pos.y },
            player.radius,
            color,
            true,
            2.0,
        );

        let text_pos = Vec2 {
            x: pos.x - player.radius,
            y: pos.y - player.radius - 12.0,
        };
        let _ = graphics.reducers.draw_text(
            LAYER.to_string(),
            player.name.clone(),
            text_pos,
            14.0,
            Color {
                r: 0.9,
                g: 0.9,
                b: 0.9,
                a: 1.0,
            },
            None,
        );
    }
}

fn world_to_screen(p: &AgarVec2, camera: &AgarVec2) -> Vec2 {
    Vec2 {
        x: (p.x - camera.x) + CAMERA_W * 0.5,
        y: (p.y - camera.y) + CAMERA_H * 0.5,
    }
}

fn color_from_id(id: &str) -> Color {
    let mut hash = 0u32;
    for b in id.bytes() {
        hash = hash
            .wrapping_mul(1664525)
            .wrapping_add(1013904223)
            .wrapping_add(b as u32);
    }
    let r = ((hash >> 0) & 0xFF) as f32 / 255.0;
    let g = ((hash >> 8) & 0xFF) as f32 / 255.0;
    let b = ((hash >> 16) & 0xFF) as f32 / 255.0;
    Color { r, g, b, a: 0.9 }
}
