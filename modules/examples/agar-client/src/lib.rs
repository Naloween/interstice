use crate::bindings::{
    agar_server::{
        HasAgarServerModuleHandle,
        agar_server::{HasFoodHandle, HasPlayerHandle, Vec2 as AgarVec2},
    },
    graphics::*,
    input::*,
    *,
};
use interstice_sdk::{key_code::KeyCode, *};

interstice_module!(
    visibility: Public,
    replicated_tables: [
        "agar-server.agar-server.player",
        "agar-server.agar-server.food",
    ]
);

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

#[reducer(on = "agar-server.agar-server.player.sync")]
pub fn on_player_sync(ctx: ReducerContext) {
    ctx.log("Player table synced");
}

#[reducer(on = "graphics.frametick.update")]
pub fn on_frame(ctx: ReducerContext, _prev: FrameTick, tick: FrameTick) {
    let server = ctx.agar_server().agar_server();
    let profile = tick.frame % 60 == 0;
    let frame_start = if profile {
        ctx.time_now_ms().ok()
    } else {
        None
    };

    let input_start = if profile {
        ctx.time_now_ms().ok()
    } else {
        None
    };
    let (dx, dy) = input_dir(&ctx);
    let input_end = if profile {
        ctx.time_now_ms().ok()
    } else {
        None
    };

    let set_dir_start = if profile {
        ctx.time_now_ms().ok()
    } else {
        None
    };
    if let Err(err) = server.reducers.set_direction(dx, dy) {
        ctx.log(&format!("set_direction failed: {}", err));
    }
    let set_dir_end = if profile {
        ctx.time_now_ms().ok()
    } else {
        None
    };

    let render_start = if profile {
        ctx.time_now_ms().ok()
    } else {
        None
    };
    let (player_count, food_count, draw_calls, graphics_api_calls) = render_world(&ctx);
    let render_end = if profile {
        ctx.time_now_ms().ok()
    } else {
        None
    };

    if profile {
        let frame_end = ctx.time_now_ms().ok();
        let input_ms = elapsed_ms(input_start, input_end);
        let set_dir_ms = elapsed_ms(set_dir_start, set_dir_end);
        let render_ms = elapsed_ms(render_start, render_end);
        let total_ms = elapsed_ms(frame_start, frame_end);

        ctx.log(&format!(
            "perf frame={} total={}ms input={}ms set_dir={}ms render={}ms players={} foods={} draws={} gfx_calls={}",
            tick.frame, total_ms, input_ms, set_dir_ms, render_ms, player_count, food_count, draw_calls, graphics_api_calls
        ));
    }
}

fn input_dir(ctx: &ReducerContext) -> (f32, f32) {
    let input = ctx.input();
    let key_states = input.tables.keystate().scan();

    // Helper to check if a key is pressed
    let pressed = |code: KeyCode| {
        key_states
            .iter()
            .any(|k| k.code == (code.clone() as u32) && k.pressed)
    };

    let mut dx: f32 = 0.0;
    let mut dy: f32 = 0.0;
    if pressed(KeyCode::KeyA) || pressed(KeyCode::ArrowLeft) {
        dx -= 1.0; // A or Left
    }
    if pressed(KeyCode::KeyD) || pressed(KeyCode::ArrowRight) {
        dx += 1.0; // D or Right
    }
    if pressed(KeyCode::KeyW) || pressed(KeyCode::ArrowUp) {
        dy -= 1.0; // W or Up
    }
    if pressed(KeyCode::KeyS) || pressed(KeyCode::ArrowDown) {
        dy += 1.0; // S or Down
    }

    let len: f32 = (dx * dx + dy * dy).sqrt();
    if len > 0.0001 {
        (dx / len, dy / len)
    } else {
        (0.0, 0.0)
    }
}

fn render_world(ctx: &ReducerContext) -> (usize, usize, usize, usize) {
    let server = ctx.agar_server().agar_server();
    let players = server.tables.player().scan();
    let foods = server.tables.food().scan();
    let mut draw_calls = 0usize;
    let mut graphics_api_calls = 0usize;

    let graphics = ctx.graphics();
    let camera = players
        .iter()
        .find(|p| p.id == ctx.current_node_id())
        .map(|p| AgarVec2 {
            x: p.pos.x,
            y: p.pos.y,
        })
        .unwrap_or(AgarVec2 { x: 0.0, y: 0.0 });

    let mut food_centers = Vec::with_capacity(foods.len());
    let mut food_radii = Vec::with_capacity(foods.len());
    for food in &foods {
        food_centers.push(world_to_screen(&food.pos, &camera));
        food_radii.push(food.radius);
        draw_calls += 1;
    }
    if !food_centers.is_empty() {
        let _ = graphics.reducers.draw_circles(
            LAYER.to_string(),
            food_centers,
            food_radii,
            Color {
                r: 0.3,
                g: 0.85,
                b: 0.45,
                a: 1.0,
            },
            true,
            0.0,
        );
        graphics_api_calls += 1;
    }

    for player in &players {
        let pos = world_to_screen(&player.pos, &camera);
        let color = color_from_id(&player.id);
        let _ = graphics.reducers.draw_circle(
            LAYER.to_string(),
            Vec2 { x: pos.x, y: pos.y },
            player.radius,
            color,
            true,
            2.0,
        );
        graphics_api_calls += 1;
        draw_calls += 1;

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
        graphics_api_calls += 1;
        draw_calls += 1;
    }

    (players.len(), foods.len(), draw_calls, graphics_api_calls)
}

fn elapsed_ms(start: Option<u64>, end: Option<u64>) -> u64 {
    match (start, end) {
        (Some(start), Some(end)) if end >= start => end - start,
        _ => 0,
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
