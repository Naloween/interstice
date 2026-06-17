use crate::bindings::{graphics::*, input::*, module_manager::*};
use interstice_sdk::*;

interstice_module!(visibility: Public);

// The desktop is a tiny surface-compositor "OS". It bakes its apps in directly
// (bytes can't ride a CLI string reducer arg), registers them with the module
// manager, and owns the screen: it claims the graphics compositor, gives each
// open app its own offscreen surface, and composites those surfaces into
// draggable / resizable / focusable windows. An app launcher overlay lists the
// installed apps (with icons) and opens/closes them.
const UI_EXAMPLE_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../crates/interstice-cli/module_examples/ui_example.wasm"
));
const AGAR_CLIENT_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../crates/interstice-cli/module_examples/agar_client.wasm"
));

/// Schema name of a bundled app (also its surface-routing key).
const APP_UI_EXAMPLE: &str = "ui-example";
/// The agar.io-style client. NOTE: it replicates tables from a separate
/// `agar-server-example` node (bound to 127.0.0.1:8086), so opening it only
/// works while `cargo run -p interstice-cli -- example agar-server` is running.
const APP_AGAR_CLIENT: &str = "agar-client";
const ICON_PX: u32 = 64;

/// The swapchain-facing layer the desktop draws its background + window chrome
/// into (z=0, clearing).
const DESKTOP_LAYER: &str = "desktop";
/// The launcher overlay + persistent top bar (z=900, non-clearing) — drawn above
/// all window composites in DESKTOP_LAYER.
const LAUNCHER_LAYER: &str = "desktop_launcher";
/// Top-most cursor layer (z=1000, non-clearing). Within a layer the renderer
/// composites `draw_surface` after all immediate geometry, so a cursor drawn
/// into a lower layer would sit *behind* a window; its own high-z layer keeps
/// it above everything.
const CURSOR_LAYER: &str = "desktop_cursor";

/// Base z for per-window layers. Each window gets two layers ranked by focus
/// order: a *content* layer (backdrop + the app's composited surface) and a
/// *chrome* layer one z above it (titlebar, title, close box, resize grip,
/// border). Two layers are required because within a single layer the renderer
/// always composites `draw_surface` *after* all immediate geometry — so chrome
/// drawn in the same layer as the surface would end up behind the content, and a
/// lower window's surface would cover a higher window's frame. Separate, z-ranked
/// layers make stacking correct: backdrop < content < chrome, per window, in
/// focus order. Window z bands live between DESKTOP_LAYER (0) and LAUNCHER (900).
const WIN_Z_BASE: i32 = 10;

const TOPBAR_H: f32 = 36.0;
const TITLEBAR_H: f32 = 32.0;
const RESIZE_HANDLE: f32 = 18.0;
const MIN_W: f32 = 240.0;
const MIN_H: f32 = 160.0;

/// A window owned by the desktop, wrapping one app's offscreen surface. `id`
/// doubles as the app's surface id. `x/y/w/h` is the outer window rect (titlebar
/// included); the content rect is `w` × `h - TITLEBAR_H`. `z` is the live focus
/// order (higher = on top / most recently focused).
#[table]
pub struct Window {
    #[primary_key]
    id: u32,
    /// Schema name of the module whose layers feed `surface_id`.
    app_name: String,
    /// Offscreen surface the app renders into (equals `id`).
    surface_id: u32,
    title: String,
    z: i32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    /// Current pixel size of `surface_id`; tracked so we resize only on change.
    surf_w: u32,
    surf_h: u32,
}

/// Singleton (`id` always 0) desktop bookkeeping.
#[table]
pub struct DesktopState {
    #[primary_key]
    id: u32,
    show_launcher: bool,
    /// Next surface id (and window id) to hand out when opening an app.
    next_surface_id: u32,
    /// Next focus-order value to assign when a window is raised.
    next_z: i32,
}

/// Singleton (`id` always 0) drag/resize state. `mode`: 0 none, 1 move, 2 resize.
/// `grab_dx/dy` is the cursor offset from the window origin captured on press.
#[table]
pub struct Interaction {
    #[primary_key]
    id: u32,
    mode: u32,
    window_id: u32,
    grab_dx: f32,
    grab_dy: f32,
}

/// Singleton (`id` always 0). `MouseButton` is ephemeral/level state, so we diff
/// against the previous frame's left-button state to derive click edges.
#[table]
pub struct PrevMouse {
    #[primary_key]
    id: u32,
    left_down: bool,
}

#[reducer(on = "load")]
pub fn on_load<Caps>(ctx: ReducerContext<Caps>)
where
    Caps: CanInsert<DesktopState> + CanInsert<Interaction> + CanInsert<PrevMouse>,
{
    ctx.log("desktop: claiming compositor and provisioning layers");
    let g = ctx.graphics();
    let _ = g.reducers.claim_compositor();
    // Background + window chrome (clears each frame).
    let _ = g.reducers.create_layer(DESKTOP_LAYER.to_string(), 0, true);
    // Launcher overlay + top bar (above windows, non-clearing).
    let _ = g
        .reducers
        .create_layer(LAUNCHER_LAYER.to_string(), 900, false);
    // Cursor (above everything, non-clearing).
    let _ = g
        .reducers
        .create_layer(CURSOR_LAYER.to_string(), 1000, false);

    // Register (don't load) the bundled apps, each with a procedurally-generated
    // icon uploaded as a texture so the launcher can `draw_image` it. Apps are
    // distinguished by a base colour. The launcher + open/close paths are generic
    // over `list_apps()`, so adding an app here is all that's needed.
    register_app_with_icon(
        &ctx,
        APP_UI_EXAMPLE,
        UI_EXAMPLE_BYTES,
        make_icon((60.0, 70.0, 160.0)),
    );
    register_app_with_icon(
        &ctx,
        APP_AGAR_CLIENT,
        AGAR_CLIENT_BYTES,
        make_icon((40.0, 150.0, 70.0)),
    );

    // Seed singletons. Apps start closed — the launcher is shown first.
    let _ = ctx.current.tables.desktopstate().insert(DesktopState {
        id: 0,
        show_launcher: true,
        next_surface_id: 1,
        next_z: 1,
    });
    let _ = ctx.current.tables.interaction().insert(Interaction {
        id: 0,
        mode: 0,
        window_id: 0,
        grab_dx: 0.0,
        grab_dy: 0.0,
    });
    let _ = ctx.current.tables.prevmouse().insert(PrevMouse {
        id: 0,
        left_down: false,
    });
}

#[reducer(on = "graphics.frametick.update")]
pub fn on_frame<Caps>(ctx: ReducerContext<Caps>, _prev: FrameTick, _tick: FrameTick)
where
    Caps: CanRead<Window>
        + CanUpdate<Window>
        + CanInsert<Window>
        + CanDelete<Window>
        + CanRead<DesktopState>
        + CanUpdate<DesktopState>
        + CanRead<Interaction>
        + CanUpdate<Interaction>
        + CanRead<PrevMouse>
        + CanUpdate<PrevMouse>
        + CanRead<MouseState>
        + CanRead<MouseButton>,
{
    // Our chrome layers resolve to the swapchain (we're unassigned), so this is
    // the real screen size — surface 0 is 0×0 until the first render.
    let info = match ctx.graphics().queries.surface_info() {
        Ok(info) => info,
        Err(_) => return,
    };
    let (sw, sh) = (info.width as f32, info.height as f32);
    if sw < 1.0 || sh < 1.0 {
        return;
    }

    // --- Read input + derive click edges -----------------------------------
    let (mx, my) = ctx
        .input()
        .tables
        .mousestate()
        .get(0)
        .map(|m| m.position)
        .unwrap_or((0.0, 0.0));
    let left_down = ctx
        .input()
        .tables
        .mousebutton()
        .get(0)
        .map(|b| b.pressed)
        .unwrap_or(false);
    let prev_down = ctx
        .current
        .tables
        .prevmouse()
        .get(0)
        .map(|p| p.left_down)
        .unwrap_or(false);
    let pressed_edge = left_down && !prev_down;
    let released_edge = !left_down && prev_down;

    let mut state = ctx
        .current
        .tables
        .desktopstate()
        .get(0)
        .unwrap_or(DesktopState {
            id: 0,
            show_launcher: true,
            next_surface_id: 1,
            next_z: 1,
        });
    let mut interaction = ctx
        .current
        .tables
        .interaction()
        .get(0)
        .unwrap_or(Interaction {
            id: 0,
            mode: 0,
            window_id: 0,
            grab_dx: 0.0,
            grab_dy: 0.0,
        });

    // Working copy of the windows; mutated below and written back once at the end
    // (avoids multiple updates of the same row within one reducer run). `existing`
    // records which ids were already committed so the writeback inserts freshly
    // opened windows and updates the rest.
    let mut windows: Vec<Window> = ctx.current.tables.window().scan();
    let existing: std::collections::HashSet<u32> = windows.iter().map(|w| w.id).collect();

    // --- Continuous drag / resize (while the button is held) ----------------
    if left_down && interaction.mode != 0 {
        if let Some(win) = windows.iter_mut().find(|w| w.id == interaction.window_id) {
            match interaction.mode {
                1 => {
                    // Move: keep the titlebar below the top bar so it stays grabbable.
                    win.x = mx - interaction.grab_dx;
                    win.y = (my - interaction.grab_dy).max(TOPBAR_H);
                }
                2 => {
                    win.w = (mx - win.x).max(MIN_W);
                    win.h = (my - win.y).max(MIN_H);
                }
                _ => {}
            }
        }
    }

    // --- Click handling -----------------------------------------------------
    let mut closed_id: Option<u32> = None;
    let mut open_app: Option<String> = None;
    if pressed_edge {
        // 1) Top-bar "Apps" button toggles the launcher.
        if in_rect(mx, my, 8.0, 4.0, 80.0, TOPBAR_H - 8.0) {
            state.show_launcher = !state.show_launcher;
        } else if state.show_launcher && my > TOPBAR_H {
            // 2) A launcher tile opens (or focuses) that app.
            let apps = list_apps_sorted(&ctx);
            for (name, _loaded, tile) in launcher_tiles(&apps, sw) {
                if in_rect(mx, my, tile.x, tile.y, tile.w, tile.h) {
                    open_app = Some(name);
                    break;
                }
            }
        } else {
            // 3) Window interaction — test top-most (highest z) first.
            let mut order: Vec<usize> = (0..windows.len()).collect();
            order.sort_by(|&a, &b| windows[b].z.cmp(&windows[a].z));
            for &i in &order {
                let win = &windows[i];
                if !in_rect(mx, my, win.x, win.y, win.w, win.h) {
                    continue;
                }
                // Close box (right end of the titlebar).
                if in_rect(mx, my, win.x + win.w - TITLEBAR_H, win.y, TITLEBAR_H, TITLEBAR_H) {
                    closed_id = Some(win.id);
                }
                // Raise focus regardless of what was hit inside the window.
                let win_id = win.id;
                let raise_z = state.next_z;
                state.next_z += 1;
                // Bottom-right resize handle.
                let on_handle = in_rect(
                    mx,
                    my,
                    win.x + win.w - RESIZE_HANDLE,
                    win.y + win.h - RESIZE_HANDLE,
                    RESIZE_HANDLE,
                    RESIZE_HANDLE,
                );
                let on_titlebar = my < win.y + TITLEBAR_H;
                let (wx, wy) = (win.x, win.y);
                let win = &mut windows[i];
                win.z = raise_z;
                if closed_id.is_none() {
                    if on_handle {
                        interaction = Interaction {
                            id: 0,
                            mode: 2,
                            window_id: win_id,
                            grab_dx: 0.0,
                            grab_dy: 0.0,
                        };
                    } else if on_titlebar {
                        interaction = Interaction {
                            id: 0,
                            mode: 1,
                            window_id: win_id,
                            grab_dx: mx - wx,
                            grab_dy: my - wy,
                        };
                    }
                }
                break;
            }
        }
    }
    if released_edge {
        interaction.mode = 0;
    }

    // --- Open an app (load + provision a surface + window) ------------------
    if let Some(name) = open_app {
        let g = ctx.graphics();
        let mm = ctx.module_manager();
        if let Some(win) = windows.iter_mut().find(|w| w.app_name == name) {
            // Already open: just raise it.
            win.z = state.next_z;
            state.next_z += 1;
        } else {
            let sid = state.next_surface_id;
            state.next_surface_id += 1;
            // Cascade new windows so they don't perfectly overlap.
            let offset = (sid as f32 - 1.0) * 28.0;
            let wx = 120.0 + offset;
            let wy = TOPBAR_H + 40.0 + offset;
            let ww = 960.0_f32.min(sw - wx - 40.0).max(MIN_W);
            let wh = 600.0_f32.min(sh - wy - 40.0).max(MIN_H);
            let content_w = ww.max(1.0) as u32;
            let content_h = (wh - TITLEBAR_H).max(1.0) as u32;

            let _ = mm.reducers.load_app(name.clone());
            let _ = g.reducers.create_surface(sid, content_w, content_h);
            let _ = g.reducers.assign_module_surface(name.clone(), sid);
            // Per-window content/chrome layers (z assigned each frame by rank).
            let _ = g.reducers.create_layer(win_content_layer(sid), WIN_Z_BASE, false);
            let _ = g.reducers.create_layer(win_chrome_layer(sid), WIN_Z_BASE + 1, false);

            windows.push(Window {
                id: sid,
                app_name: name.clone(),
                surface_id: sid,
                title: name.clone(),
                z: state.next_z,
                x: wx,
                y: wy,
                w: ww,
                h: wh,
                surf_w: content_w,
                surf_h: content_h,
            });
            state.next_z += 1;
        }
        state.show_launcher = false;
    }

    // --- Close an app (unload + tear down the surface + window) -------------
    if let Some(id) = closed_id {
        if let Some(win) = windows.iter().find(|w| w.id == id) {
            let name = win.app_name.clone();
            let surface_id = win.surface_id;
            let g = ctx.graphics();
            let mm = ctx.module_manager();
            let _ = mm.reducers.unload_app(name.clone());
            let _ = g.reducers.assign_module_surface(name, 0);
            let _ = g.reducers.destroy_surface(surface_id);
            let _ = g.reducers.destroy_layer(win_content_layer(surface_id));
            let _ = g.reducers.destroy_layer(win_chrome_layer(surface_id));
        }
        interaction.mode = 0;
    }

    // --- Draw: background ---------------------------------------------------
    let g = ctx.graphics();
    let _ = g.reducers.draw_rect(
        DESKTOP_LAYER.to_string(),
        Rect { x: 0.0, y: 0.0, w: sw, h: sh },
        Color { r: 0.07, g: 0.08, b: 0.12, a: 1.0 },
        true,
        0.0,
        None,
    );

    // --- Draw: windows (sorted bottom-to-top by z) -------------------------
    let mut draw_order: Vec<usize> = (0..windows.len())
        .filter(|&i| Some(windows[i].id) != closed_id)
        .collect();
    draw_order.sort_by(|&a, &b| windows[a].z.cmp(&windows[b].z));
    for (rank, &i) in draw_order.iter().enumerate() {
        // Resize the app's surface to the content rect's native resolution so the
        // composite is 1:1 (no stretch). Only on change.
        let content_w = windows[i].w.max(1.0) as u32;
        let content_h = (windows[i].h - TITLEBAR_H).max(1.0) as u32;
        if windows[i].surf_w != content_w || windows[i].surf_h != content_h {
            let _ = g
                .reducers
                .resize_surface(windows[i].surface_id, content_w, content_h);
            windows[i].surf_w = content_w;
            windows[i].surf_h = content_h;
        }
        let win = &windows[i];

        // Per-window layers, z-ranked so a higher window's content/chrome always
        // sits above a lower window's. Content (backdrop + surface) below; chrome
        // (titlebar/title/close/resize/border) one z above so it draws on top of
        // the composited surface rather than behind it.
        let content_layer = win_content_layer(win.surface_id);
        let chrome_layer = win_chrome_layer(win.surface_id);
        let base_z = WIN_Z_BASE + (rank as i32) * 2;
        let _ = g.reducers.set_layer_z(content_layer.clone(), base_z);
        let _ = g.reducers.set_layer_z(chrome_layer.clone(), base_z + 1);

        // Window backdrop (content layer, below the app surface).
        let _ = g.reducers.draw_rect(
            content_layer.clone(),
            Rect { x: win.x, y: win.y, w: win.w, h: win.h },
            Color { r: 0.15, g: 0.16, b: 0.20, a: 1.0 },
            true,
            0.0,
            Some(8.0),
        );
        // Composite the app's offscreen surface into the content area.
        let content = Rect {
            x: win.x,
            y: win.y + TITLEBAR_H,
            w: win.w,
            h: win.h - TITLEBAR_H,
        };
        let _ = g.reducers.draw_surface(
            content_layer,
            win.surface_id,
            content,
            Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
        );

        // Titlebar (chrome layer, above the surface).
        let _ = g.reducers.draw_rect(
            chrome_layer.clone(),
            Rect { x: win.x, y: win.y, w: win.w, h: TITLEBAR_H },
            Color { r: 0.22, g: 0.24, b: 0.32, a: 1.0 },
            true,
            0.0,
            Some(8.0),
        );
        // Title text.
        let _ = g.reducers.draw_text(
            chrome_layer.clone(),
            win.title.clone(),
            Vec2 { x: win.x + 12.0, y: win.y + 8.0 },
            15.0,
            Color { r: 0.90, g: 0.92, b: 0.96, a: 1.0 },
            None,
        );
        // Close box.
        let _ = g.reducers.draw_rect(
            chrome_layer.clone(),
            Rect {
                x: win.x + win.w - TITLEBAR_H + 6.0,
                y: win.y + 6.0,
                w: TITLEBAR_H - 12.0,
                h: TITLEBAR_H - 12.0,
            },
            Color { r: 0.80, g: 0.30, b: 0.32, a: 1.0 },
            true,
            0.0,
            Some(4.0),
        );
        // Resize handle (bottom-right grip).
        let _ = g.reducers.draw_rect(
            chrome_layer.clone(),
            Rect {
                x: win.x + win.w - RESIZE_HANDLE,
                y: win.y + win.h - RESIZE_HANDLE,
                w: RESIZE_HANDLE,
                h: RESIZE_HANDLE,
            },
            Color { r: 0.45, g: 0.48, b: 0.58, a: 0.9 },
            true,
            0.0,
            Some(3.0),
        );
        // Border on top.
        let _ = g.reducers.draw_rect(
            chrome_layer,
            Rect { x: win.x, y: win.y, w: win.w, h: win.h },
            Color { r: 0.35, g: 0.38, b: 0.48, a: 1.0 },
            false,
            2.0,
            Some(8.0),
        );
    }

    // --- Draw: top bar + launcher overlay ----------------------------------
    // Persistent top bar with the "Apps" toggle.
    let _ = g.reducers.draw_rect(
        LAUNCHER_LAYER.to_string(),
        Rect { x: 0.0, y: 0.0, w: sw, h: TOPBAR_H },
        Color { r: 0.10, g: 0.11, b: 0.16, a: 0.92 },
        true,
        0.0,
        None,
    );
    let _ = g.reducers.draw_rect(
        LAUNCHER_LAYER.to_string(),
        Rect { x: 8.0, y: 4.0, w: 80.0, h: TOPBAR_H - 8.0 },
        if state.show_launcher {
            Color { r: 0.30, g: 0.44, b: 0.78, a: 1.0 }
        } else {
            Color { r: 0.20, g: 0.22, b: 0.30, a: 1.0 }
        },
        true,
        0.0,
        Some(6.0),
    );
    let _ = g.reducers.draw_text(
        LAUNCHER_LAYER.to_string(),
        "Apps".to_string(),
        Vec2 { x: 24.0, y: 11.0 },
        14.0,
        Color { r: 0.92, g: 0.94, b: 0.98, a: 1.0 },
        None,
    );

    if state.show_launcher {
        // Dim panel over the whole screen (below the top bar).
        let _ = g.reducers.draw_rect(
            LAUNCHER_LAYER.to_string(),
            Rect { x: 0.0, y: TOPBAR_H, w: sw, h: sh - TOPBAR_H },
            Color { r: 0.04, g: 0.05, b: 0.08, a: 0.82 },
            true,
            0.0,
            None,
        );
        let apps = list_apps_sorted(&ctx);
        for (name, loaded, tile) in launcher_tiles(&apps, sw) {
            let (tx, ty, tw, th) = (tile.x, tile.y, tile.w, tile.h);
            // Tile background.
            let _ = g.reducers.draw_rect(
                LAUNCHER_LAYER.to_string(),
                Rect { x: tx, y: ty, w: tw, h: th },
                Color { r: 0.16, g: 0.18, b: 0.24, a: 0.95 },
                true,
                0.0,
                Some(10.0),
            );
            // Icon (per-app texture; Phase B draws it, skips gracefully if absent).
            let icon_size = 64.0_f32;
            let icon_rect = Rect {
                x: tx + (tw - icon_size) / 2.0,
                y: ty + 12.0,
                w: icon_size,
                h: icon_size,
            };
            let _ = g.reducers.draw_image(
                LAUNCHER_LAYER.to_string(),
                icon_texture_id(&name),
                icon_rect,
                Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
                // Draw the whole icon texture (no cropping).
                Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 },
            );
            // Name label + a "running" dot.
            let _ = g.reducers.draw_text(
                LAUNCHER_LAYER.to_string(),
                name.clone(),
                Vec2 { x: tx + 10.0, y: ty + th - 24.0 },
                13.0,
                Color { r: 0.90, g: 0.92, b: 0.96, a: 1.0 },
                None,
            );
            if loaded {
                let _ = g.reducers.draw_circle(
                    LAUNCHER_LAYER.to_string(),
                    Vec2 { x: tx + tw - 14.0, y: ty + 14.0 },
                    5.0,
                    Color { r: 0.30, g: 0.80, b: 0.40, a: 1.0 },
                    true,
                    0.0,
                );
            }
        }
    }

    // --- Draw: cursor (top-most layer) -------------------------------------
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

    // --- Write back state --------------------------------------------------
    for win in &windows {
        if Some(win.id) == closed_id {
            continue;
        }
        if existing.contains(&win.id) {
            let _ = ctx.current.tables.window().update(win.clone());
        } else {
            let _ = ctx.current.tables.window().insert(win.clone());
        }
    }
    if let Some(id) = closed_id {
        let _ = ctx.current.tables.window().delete(id);
    }
    let _ = ctx.current.tables.desktopstate().update(state);
    let _ = ctx.current.tables.interaction().update(interaction);
    let _ = ctx.current.tables.prevmouse().update(PrevMouse {
        id: 0,
        left_down,
    });
}

#[reducer]
pub fn unload(ctx: ReducerContext) {
    ctx.log("desktop: unloading open apps via module_manager");
    let mm = ctx.module_manager();
    let _ = mm.reducers.unload_app(APP_UI_EXAMPLE.to_string());
    let _ = mm.reducers.unload_app(APP_AGAR_CLIENT.to_string());
}

/// Texture `local_id` convention for an app's launcher icon.
fn icon_texture_id(app_name: &str) -> String {
    format!("icon_{app_name}")
}

/// Per-window layer names (content below, chrome above). See [`WIN_Z_BASE`].
fn win_content_layer(id: u32) -> String {
    format!("win_content_{id}")
}
fn win_chrome_layer(id: u32) -> String {
    format!("win_chrome_{id}")
}

/// Returns the installed apps in a stable order (by id) for the launcher grid.
fn list_apps_sorted<Caps>(ctx: &ReducerContext<Caps>) -> Vec<AppInfo> {
    let mut apps = ctx
        .module_manager()
        .queries
        .list_apps()
        .unwrap_or_default();
    apps.sort_by_key(|a| a.id);
    apps
}

/// Computes the launcher tile `(app name, loaded, rect)` for each app, laid out
/// in a centered grid starting below the top bar. Returns owned values because
/// the binding's `AppInfo`/`Rect` aren't `Clone`/`Copy`.
fn launcher_tiles(apps: &[AppInfo], sw: f32) -> Vec<(String, bool, Rect)> {
    const TILE: f32 = 120.0;
    const GAP: f32 = 24.0;
    const MARGIN: f32 = 40.0;
    let usable = (sw - 2.0 * MARGIN).max(TILE);
    let cols = (((usable + GAP) / (TILE + GAP)).floor() as usize).max(1);
    let grid_w = cols as f32 * TILE + (cols as f32 - 1.0) * GAP;
    let start_x = (sw - grid_w).max(MARGIN) / 2.0;
    let start_y = TOPBAR_H + 32.0;

    apps.iter()
        .enumerate()
        .map(|(i, app)| {
            let col = i % cols;
            let row = i / cols;
            let rect = Rect {
                x: start_x + col as f32 * (TILE + GAP),
                y: start_y + row as f32 * (TILE + GAP),
                w: TILE,
                h: TILE,
            };
            (app.name.clone(), app.loaded, rect)
        })
        .collect()
}

fn in_rect(px: f32, py: f32, x: f32, y: f32, w: f32, h: f32) -> bool {
    px >= x && px <= x + w && py >= y && py <= y + h
}

/// Registers an app with the module manager (without loading it) and uploads its
/// launcher icon as a texture under the `icon_<name>` convention. Cross-module
/// reducer calls (`register_app`, `create_texture`) need no table capabilities.
fn register_app_with_icon<Caps>(ctx: &ReducerContext<Caps>, name: &str, bytes: &[u8], icon: Vec<u8>) {
    let _ = ctx.module_manager().reducers.register_app(
        name.to_string(),
        bytes.to_vec(),
        Some(icon.clone()),
    );
    let _ = ctx.graphics().reducers.create_texture(
        icon_texture_id(name),
        TextureDescriptorInput {
            width: ICON_PX,
            height: ICON_PX,
            format: "rgba8unorm".to_string(),
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
        icon,
    );
}

/// Builds an app icon as raw RGBA8 (`ICON_PX`×`ICON_PX`, 4 bytes/px): a vertical
/// gradient from `base` (top) brightening downward, with a soft white disc in the
/// middle. `base` is the per-app accent colour so tiles are distinguishable.
fn make_icon(base: (f32, f32, f32)) -> Vec<u8> {
    let n = ICON_PX as usize;
    let mut data = vec![0u8; n * n * 4];
    let c = (n as f32 - 1.0) / 2.0;
    for y in 0..n {
        for x in 0..n {
            let idx = (y * n + x) * 4;
            let t = y as f32 / n as f32;
            let mut r = (base.0 + 40.0 * t) as u8;
            let mut gg = (base.1 + 60.0 * t) as u8;
            let mut b = (base.2 + 60.0 * t) as u8;
            let dx = x as f32 - c;
            let dy = y as f32 - c;
            let d = (dx * dx + dy * dy).sqrt();
            if d < 18.0 {
                r = 240;
                gg = 244;
                b = 250;
            } else if d < 22.0 {
                r = 120;
                gg = 140;
                b = 200;
            }
            data[idx] = r;
            data[idx + 1] = gg;
            data[idx + 2] = b;
            data[idx + 3] = 255;
        }
    }
    data
}
