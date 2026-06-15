use crate::bindings::{graphics::*, input::*, module_manager::*};
use interstice_sdk::*;

interstice_module!(visibility: Public);

// The desktop is a tiny surface-compositor "OS". It bakes its apps in directly
// (bytes can't ride a CLI string reducer arg), asks the module manager to load
// them, and owns the screen: it claims the graphics compositor, gives each app
// its own offscreen surface, and composites those surfaces into windows.
const UI_EXAMPLE_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../crates/interstice-cli/module_examples/ui_example.wasm"
));

/// The offscreen surface the app renders into. Resized each frame to match its
/// window's content rect so the app draws at native resolution (1:1 composite,
/// no stretching).
const APP_SURFACE_ID: u32 = 1;
const SURFACE_W: u32 = 1280;
const SURFACE_H: u32 = 720;

/// The single swapchain-facing layer the desktop draws all its chrome into.
const DESKTOP_LAYER: &str = "desktop";
/// A dedicated top-most layer for the cursor. Within a layer the renderer
/// composites `draw_surface` (the app's window) after all immediate geometry,
/// so a cursor drawn into DESKTOP_LAYER would sit *behind* the window. Giving
/// the cursor its own higher-z, non-clearing layer keeps it above everything.
const CURSOR_LAYER: &str = "desktop_cursor";

/// A window owned by the desktop, wrapping one app's offscreen surface.
#[table]
pub struct Window {
    #[primary_key]
    id: u32,
    /// Schema name of the module whose layers feed `surface_id`.
    app_name: String,
    /// Offscreen surface the app renders into.
    surface_id: u32,
    title: String,
    z: i32,
    /// Current size of `surface_id`; tracked so we resize only on change.
    surf_w: u32,
    surf_h: u32,
}

#[reducer(on = "load")]
pub fn on_load<Caps>(ctx: ReducerContext<Caps>)
where
    Caps: CanInsert<Window>,
{
    ctx.log("desktop: claiming compositor and provisioning the app surface");
    let g = ctx.graphics();
    let _ = g.reducers.claim_compositor();
    let _ = g
        .reducers
        .create_surface(APP_SURFACE_ID, SURFACE_W, SURFACE_H);
    // Route the app's (module name "ui-example") layers off the swapchain and
    // into our surface. The routing key is the app's caller_module_name.
    let _ = g
        .reducers
        .assign_module_surface("ui-example".to_string(), APP_SURFACE_ID);
    // Our own chrome layer renders straight to the swapchain (we are unassigned).
    let _ = g
        .reducers
        .create_layer(DESKTOP_LAYER.to_string(), 0, true);
    // Top-most cursor layer (non-clearing so it doesn't wipe the chrome below).
    let _ = g
        .reducers
        .create_layer(CURSOR_LAYER.to_string(), 1000, false);

    let _ = ctx.current.tables.window().insert(Window {
        id: 1,
        app_name: "ui-example".to_string(),
        surface_id: APP_SURFACE_ID,
        title: "UI Example".to_string(),
        z: 0,
        surf_w: SURFACE_W,
        surf_h: SURFACE_H,
    });

    ctx.log("desktop: launching 'ui-example' app via module_manager");
    let mm = ctx.module_manager();
    let _ = mm
        .reducers
        .load("ui-example".to_string(), UI_EXAMPLE_BYTES.to_vec(), None);
}

#[reducer(on = "graphics.frametick.update")]
pub fn on_frame<Caps>(ctx: ReducerContext<Caps>, _prev: FrameTick, _tick: FrameTick)
where
    Caps: CanRead<Window> + CanUpdate<Window> + CanRead<MouseState>,
{
    // Our own surface resolves to the swapchain (we're unassigned), so this gives
    // the real screen size each frame — surface 0 is 0×0 until the first render.
    let info = match ctx.graphics().queries.surface_info() {
        Ok(info) => info,
        Err(_) => return,
    };
    let (sw, sh) = (info.width as f32, info.height as f32);
    if sw < 1.0 || sh < 1.0 {
        return;
    }

    let g = ctx.graphics();

    // Desktop background.
    let _ = g.reducers.draw_rect(
        DESKTOP_LAYER.to_string(),
        Rect { x: 0.0, y: 0.0, w: sw, h: sh },
        Color { r: 0.07, g: 0.08, b: 0.12, a: 1.0 },
        true,
        0.0,
        None,
    );

    let margin = 60.0_f32;
    let titlebar_h = 36.0_f32;

    for mut win in ctx.current.tables.window().scan() {
        let win_x = margin;
        let win_y = margin;
        let win_w = sw - 2.0 * margin;
        let win_h = sh - 2.0 * margin;
        if win_w < 1.0 || win_h < 1.0 {
            continue;
        }

        // The app draws into its surface at the content rect's native resolution
        // so the composite is 1:1 (no stretching). Resize only when it changes.
        let content_w = win_w.max(1.0) as u32;
        let content_h = (win_h - titlebar_h).max(1.0) as u32;
        if win.surf_w != content_w || win.surf_h != content_h {
            let _ = g
                .reducers
                .resize_surface(win.surface_id, content_w, content_h);
            win.surf_w = content_w;
            win.surf_h = content_h;
            let _ = ctx.current.tables.window().update(win.clone());
        }

        // Window backdrop.
        let _ = g.reducers.draw_rect(
            DESKTOP_LAYER.to_string(),
            Rect { x: win_x, y: win_y, w: win_w, h: win_h },
            Color { r: 0.15, g: 0.16, b: 0.20, a: 1.0 },
            true,
            0.0,
            Some(8.0),
        );
        // Titlebar.
        let _ = g.reducers.draw_rect(
            DESKTOP_LAYER.to_string(),
            Rect { x: win_x, y: win_y, w: win_w, h: titlebar_h },
            Color { r: 0.22, g: 0.24, b: 0.32, a: 1.0 },
            true,
            0.0,
            Some(8.0),
        );
        // Title text.
        let _ = g.reducers.draw_text(
            DESKTOP_LAYER.to_string(),
            win.title.clone(),
            Vec2 { x: win_x + 12.0, y: win_y + 9.0 },
            16.0,
            Color { r: 0.90, g: 0.92, b: 0.96, a: 1.0 },
            None,
        );
        // Composite the app's offscreen surface into the content area (scaled).
        let content = Rect {
            x: win_x,
            y: win_y + titlebar_h,
            w: win_w,
            h: win_h - titlebar_h,
        };
        let _ = g.reducers.draw_surface(
            DESKTOP_LAYER.to_string(),
            win.surface_id,
            content,
            Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
        );
        // Window border on top.
        let _ = g.reducers.draw_rect(
            DESKTOP_LAYER.to_string(),
            Rect { x: win_x, y: win_y, w: win_w, h: win_h },
            Color { r: 0.35, g: 0.38, b: 0.48, a: 1.0 },
            false,
            2.0,
            Some(8.0),
        );

        let _ = win.z; // reserved for z-ordering in Phase 4
    }

    // The desktop owns the global cursor: apps no longer draw their own. Drawn
    // last so it sits above all window chrome.
    if let Some((mx, my)) = ctx.input().tables.mousestate().get(0).map(|m| m.position) {
        let _ = g.reducers.draw_circle(
            CURSOR_LAYER.to_string(),
            Vec2 { x: mx, y: my },
            7.0,
            Color { r: 0.0, g: 0.0, b: 0.0, a: 0.5 },
            true,
            0.0,
        );
        let _ = g.reducers.draw_circle(
            CURSOR_LAYER.to_string(),
            Vec2 { x: mx, y: my },
            6.0,
            Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
            true,
            0.0,
        );
    }
}

#[reducer]
pub fn unload(ctx: ReducerContext) {
    ctx.log("desktop: unloading 'ui-example' app via module_manager");
    let mm = ctx.module_manager();
    let _ = mm.reducers.unload_app("ui-example".to_string());
}
