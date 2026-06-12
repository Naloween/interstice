use crate::bindings::{agar_server_example::*, input::*, *};
use interstice_sdk::{key_code::KeyCode, *};

pub fn handle_ingame_input<Caps>(ctx: &ReducerContext<Caps>, sw: f32, sh: f32)
where
    Caps: CanRead<KeyState> + CanRead<MouseState>,
{
    let (dx, dy) = steering_dir(ctx, sw, sh);
    let server = ctx.agar_server_example().agar_server();
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
    if pressed(KeyCode::KeyA as u32) || pressed(KeyCode::ArrowLeft as u32) {
        dx -= sw * 0.5;
    }
    if pressed(KeyCode::KeyD as u32) || pressed(KeyCode::ArrowRight as u32) {
        dx += sw * 0.5;
    }
    if pressed(KeyCode::KeyW as u32) || pressed(KeyCode::ArrowUp as u32) {
        dy -= sh * 0.5;
    }
    if pressed(KeyCode::KeyS as u32) || pressed(KeyCode::ArrowDown as u32) {
        dy += sh * 0.5;
    }

    let len = (dx * dx + dy * dy).sqrt();
    if len > 0.001 {
        (dx / len, dy / len)
    } else {
        (0.0, 0.0)
    }
}
