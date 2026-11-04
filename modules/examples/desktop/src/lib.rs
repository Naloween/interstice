use crate::bindings::{graphics::*, input::*, module_manager::*};
use interstice_sdk::*;

interstice_module!(visibility: Public);

// The desktop: an OS made of modules (DESIGN §12.2). It holds no authority.
// It is the graphics *compositor* (each app renders into its own offscreen
// surface, composited here into windows), the input *router* (each app gets
// the pointer in its own coordinates and the keyboard only when focused), and
// a client of `module_manager` (apps are registered wasm blobs, loaded and
// unloaded at runtime, recorded and replayed).
//
// The example wasms are baked in because bytes can't ride a CLI string
// argument. `build_modules.bash` builds them before this module.

macro_rules! example_wasm {
    ($file:literal) => {
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../crates/interstice-cli/module_examples/", $file))
    };
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    /// Gets a surface, a window, input routing.
    Window,
    /// No window: its tile starts and stops it.
    Service,
}

struct AppSpec {
    /// Module name: the registry key, surface and input routing key.
    name: &'static str,
    title: &'static str,
    /// One line on the launcher tile: what the app shows.
    blurb: &'static str,
    kind: Kind,
    /// Loaded first when the app opens; can't be stopped while it runs.
    requires: &'static [&'static str],
    color: (f32, f32, f32),
    bytes: &'static [u8],
}

const APPS: &[AppSpec] = &[
    AppSpec {
        name: "ui-example",
        title: "UI demo",
        blurb: "the UI library",
        kind: Kind::Window,
        requires: &[],
        color: (60.0, 70.0, 160.0),
        bytes: example_wasm!("ui_example.wasm"),
    },
    AppSpec {
        name: "notes",
        title: "Notes",
        blurb: "time travel",
        kind: Kind::Window,
        requires: &["hello-example"],
        color: (170.0, 120.0, 40.0),
        bytes: example_wasm!("notes.wasm"),
    },
    AppSpec {
        name: "browser-example",
        title: "Browser",
        blurb: "Blitz + JS + network",
        kind: Kind::Window,
        requires: &[],
        color: (40.0, 120.0, 160.0),
        bytes: example_wasm!("browser_example.wasm"),
    },
    AppSpec {
        name: "agar-client",
        title: "Agar",
        blurb: "needs example agar-server",
        kind: Kind::Window,
        requires: &[],
        color: (40.0, 150.0, 70.0),
        bytes: example_wasm!("agar_client.wasm"),
    },
    AppSpec {
        name: "hello-example",
        title: "Hello",
        blurb: "service: notes backend",
        kind: Kind::Service,
        requires: &[],
        color: (120.0, 60.0, 150.0),
        bytes: example_wasm!("hello_example.wasm"),
    },
    AppSpec {
        name: "http-get-example",
        title: "HTTP GET",
        blurb: "service: network broker",
        kind: Kind::Service,
        requires: &[],
        color: (150.0, 70.0, 70.0),
        bytes: example_wasm!("http_get_example.wasm"),
    },
    AppSpec {
        name: "audio-example",
        title: "Audio",
        blurb: "service: 440 Hz tone",
        kind: Kind::Service,
        requires: &[],
        color: (60.0, 130.0, 130.0),
        bytes: example_wasm!("audio_example.wasm"),
    },
];

const ICON_PX: u32 = 64;

/// Background (z 0, clearing).
const DESKTOP_LAYER: &str = "desktop";
/// Top bar + launcher overlay, above every window (z 900).
const LAUNCHER_LAYER: &str = "desktop_launcher";
/// Cursor, above everything (z 1000). Within one layer `draw_surface`
/// composites after immediate geometry, so the cursor needs its own layer.
const CURSOR_LAYER: &str = "desktop_cursor";
/// Per-window layers sit between the background and the launcher: a content
/// layer (backdrop + app surface) and a chrome layer one z above it, ranked
/// by focus order. Two layers because chrome drawn in the content layer would
/// end up behind the composited surface.
const WIN_Z_BASE: i32 = 10;

const TOPBAR_H: f32 = 36.0;
const TITLEBAR_H: f32 = 32.0;
const RESIZE_HANDLE: f32 = 18.0;
const MIN_W: f32 = 240.0;
const MIN_H: f32 = 160.0;
const TOAST_MS: u64 = 6000;

/// A window wrapping one app's offscreen surface. `id` is also the surface id.
/// `x/y/w/h` is the outer rect (titlebar included); the content rect, which is
/// also the app's input route, is below the titlebar. `z` is the focus order.
#[table]
#[derive(PartialEq)]
pub struct Window {
    #[primary_key]
    id: u32,
    app_name: String,
    surface_id: u32,
    title: String,
    z: i32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    /// Current pixel size of the surface, to resize only on change.
    surf_w: u32,
    surf_h: u32,
    /// The app is being recorded (the ● button).
    recording: bool,
}

/// Singleton desktop bookkeeping. `focus` and `captured` mirror what was last
/// sent to `input`, so the routing reducers are called only on change.
#[table]
#[derive(PartialEq)]
pub struct DesktopState {
    #[primary_key]
    id: u32,
    show_launcher: bool,
    next_surface_id: u32,
    next_z: i32,
    focus: String,
    captured: bool,
    /// An app asked for by the `open` reducer, handled like a tile click on
    /// the next frame (`""` for none).
    pending_open: String,
}

/// Singleton move/resize state. `mode`: 0 none, 1 move, 2 resize.
#[table]
#[derive(PartialEq)]
pub struct Interaction {
    #[primary_key]
    id: u32,
    mode: u32,
    window_id: u32,
    grab_dx: f32,
    grab_dy: f32,
}

/// Singleton: last frame's left button, for click edges.
#[table]
pub struct PrevMouse {
    #[primary_key]
    id: u32,
    left_down: bool,
}

/// Singleton: the message shown in the top bar until `until_ms`.
#[table]
pub struct Toast {
    #[primary_key]
    id: u32,
    text: String,
    ok: bool,
    until_ms: u64,
}

#[reducer(on = "load")]
pub fn on_load<Caps>(ctx: ReducerContext<Caps>)
where
    Caps: CanInsert<DesktopState> + CanInsert<Interaction> + CanInsert<PrevMouse>,
{
    ctx.log("desktop: claiming the compositor and the input router");
    let g = ctx.graphics();
    let _ = g.reducers.claim_compositor();
    let _ = g.reducers.create_layer(DESKTOP_LAYER.to_string(), 0, true);
    let _ = g.reducers.create_layer(LAUNCHER_LAYER.to_string(), 900, false);
    let _ = g.reducers.create_layer(CURSOR_LAYER.to_string(), 1000, false);
    let i = ctx.input();
    let _ = i.reducers.claim_input_router();
    // The launcher shows first, and owns the pointer while it does.
    let _ = i.reducers.set_pointer_capture(true);

    for app in APPS {
        register_app_with_icon(&ctx, app.name, app.bytes, make_icon(app.color));
    }

    let _ = ctx.current.tables.desktopstate().insert(DesktopState {
        id: 0,
        show_launcher: true,
        next_surface_id: 1,
        next_z: 1,
        focus: String::new(),
        captured: true,
        pending_open: String::new(),
    });
    let _ = ctx.current.tables.interaction().insert(Interaction {
        id: 0,
        mode: 0,
        window_id: 0,
        grab_dx: 0.0,
        grab_dy: 0.0,
    });
    let _ = ctx.current.tables.prevmouse().insert(PrevMouse { id: 0, left_down: false });
}

/// A replay finished: show its outcome in the top bar.
#[reducer(on = "module_manager.replayreport.insert")]
pub fn on_replay_report<Caps>(ctx: ReducerContext<Caps>, report: ReplayReport) -> Result<(), String>
where
    Caps: CanRead<Toast> + CanInsert<Toast> + CanUpdate<Toast>,
{
    let first = report.message.lines().next().unwrap_or("").to_string();
    let text = if report.ok {
        format!("Replay of {} reproduced the live state: {first}", report.module)
    } else {
        format!("Replay of {} diverged: {first}", report.module)
    };
    toast(&ctx, text, report.ok)
}

/// A load finished. Loads compile in the background, so a failure (e.g.
/// agar-client while agar-server isn't running) arrives here, later: close
/// the app's window, as its close box would, and say why.
#[reducer(on = "module_manager.loadreport.insert")]
pub fn on_load_report<Caps>(ctx: ReducerContext<Caps>, report: LoadReport) -> Result<(), String>
where
    Caps: CanAll<Window> + CanRead<Toast> + CanInsert<Toast> + CanUpdate<Toast>,
{
    if report.ok {
        return toast(&ctx, format!("{} is running", report.module), true);
    }
    let windows = ctx.current.tables.window().scan()?;
    if let Some(win) = windows.iter().find(|w| !report.module.is_empty() && w.app_name == report.module) {
        ctx.current.tables.window().delete(win.id)?;
        release_window(&ctx, win);
    }
    let first = report.message.lines().next().unwrap_or("").to_string();
    toast(&ctx, first, false)
}

/// Give back what a window holds outside the desktop's tables: its
/// recording, input route, surface and layers. The caller unloads the app
/// (unless the load already failed) and deletes the `Window` row.
fn release_window<Caps>(ctx: &ReducerContext<Caps>, win: &Window) {
    let g = ctx.graphics();
    if win.recording {
        let _ = ctx.module_manager().reducers.record_stop(win.app_name.clone());
    }
    let _ = ctx.input().reducers.remove_input_route(win.app_name.clone());
    let _ = g.reducers.assign_module_surface(win.app_name.clone(), 0);
    let _ = g.reducers.destroy_surface(win.surface_id);
    let _ = g.reducers.destroy_layer(win_content_layer(win.surface_id));
    let _ = g.reducers.destroy_layer(win_chrome_layer(win.surface_id));
}

#[reducer(on = "graphics.frametick.update")]
pub fn on_frame<Caps>(ctx: ReducerContext<Caps>, _prev: FrameTick, _tick: FrameTick) -> Result<(), String>
where
    Caps: CanAll<Window>
        + CanAll<DesktopState>
        + CanAll<Interaction>
        + CanAll<PrevMouse>
        + CanRead<Toast>
        + CanInsert<Toast>
        + CanUpdate<Toast>,
{
    // We are unassigned, so this is the swapchain: the real screen size.
    let Ok(info) = ctx.graphics().queries.surface_info() else {
        return Ok(());
    };
    let (sw, sh) = (info.width as f32, info.height as f32);
    if sw < 1.0 || sh < 1.0 {
        return Ok(());
    }

    // As the router, `input_view` gives us the global pointer and the raw
    // buttons, so chrome works even when a click lands in a window.
    let view = ctx.input().queries.input_view()?;
    let (mx, my) = view.position;
    let left_down = view.left;
    let prev_mouse = ctx.current.tables.prevmouse().get(0)?;
    let prev_down = prev_mouse.as_ref().is_some_and(|p| p.left_down);
    let pressed_edge = left_down && !prev_down;
    let released_edge = !left_down && prev_down;

    let Some(state0) = ctx.current.tables.desktopstate().get(0)? else {
        return Ok(());
    };
    let Some(interaction0) = ctx.current.tables.interaction().get(0)? else {
        return Ok(());
    };
    let mut state = state0.clone();
    let mut interaction = interaction0.clone();

    // Working copy of the windows, written back once at the end (only rows
    // that changed). `before` is the committed state, for that diff.
    let before: Vec<Window> = ctx.current.tables.window().scan()?;
    let mut windows = before.clone();
    let apps = list_apps_sorted(&ctx);
    let loaded = |name: &str| apps.iter().any(|a| a.name == name && a.loaded);

    // --- Move / resize while the button is held ------------------------------
    if left_down && interaction.mode != 0 {
        if let Some(win) = windows.iter_mut().find(|w| w.id == interaction.window_id) {
            match interaction.mode {
                1 => {
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

    // --- Clicks ----------------------------------------------------------------
    let mut closed_id: Option<u32> = None;
    let mut open_app: Option<&'static AppSpec> = None;
    let mut toggle_service: Option<&'static AppSpec> = None;
    let mut record_toggle: Option<u32> = None;
    if pressed_edge {
        if in_rect(mx, my, 8.0, 4.0, 80.0, TOPBAR_H - 8.0) {
            state.show_launcher = !state.show_launcher;
        } else if state.show_launcher && my > TOPBAR_H {
            let hit = launcher_tiles(sw).into_iter().find(|(_, r)| in_rect(mx, my, r.x, r.y, r.w, r.h));
            match hit.map(|(app, _)| app) {
                Some(app) if app.kind == Kind::Window => open_app = Some(app),
                Some(app) => toggle_service = Some(app),
                // A click on the dimmed backdrop closes the launcher.
                None => state.show_launcher = false,
            }
        } else {
            // Windows, top-most first. Any click in a window raises it.
            let mut order: Vec<usize> = (0..windows.len()).collect();
            order.sort_by(|&a, &b| windows[b].z.cmp(&windows[a].z));
            if let Some(i) = order.into_iter().find(|&i| {
                let w = &windows[i];
                in_rect(mx, my, w.x, w.y, w.w, w.h)
            }) {
                let win = &mut windows[i];
                win.z = state.next_z;
                state.next_z += 1;
                let on_titlebar = my < win.y + TITLEBAR_H;
                if on_titlebar && in_rect(mx, my, close_box(win).0, win.y, TITLEBAR_H, TITLEBAR_H) {
                    closed_id = Some(win.id);
                } else if on_titlebar && in_rect(mx, my, rec_box(win).0, win.y, TITLEBAR_H, TITLEBAR_H) {
                    record_toggle = Some(win.id);
                } else if in_rect(
                    mx,
                    my,
                    win.x + win.w - RESIZE_HANDLE,
                    win.y + win.h - RESIZE_HANDLE,
                    RESIZE_HANDLE,
                    RESIZE_HANDLE,
                ) {
                    interaction = Interaction { id: 0, mode: 2, window_id: win.id, grab_dx: 0.0, grab_dy: 0.0 };
                } else if on_titlebar {
                    interaction =
                        Interaction { id: 0, mode: 1, window_id: win.id, grab_dx: mx - win.x, grab_dy: my - win.y };
                }
            }
        }
    }
    if released_edge {
        interaction.mode = 0;
    }
    if !state.pending_open.is_empty() {
        match APPS.iter().find(|a| a.name == state.pending_open) {
            Some(app) if app.kind == Kind::Window => open_app = open_app.or(Some(app)),
            Some(app) => toggle_service = toggle_service.or(Some(app)),
            None => ctx.log(&format!("desktop: unknown app '{}'", state.pending_open)),
        }
        state.pending_open.clear();
    }

    let g = ctx.graphics();
    let mm = ctx.module_manager();
    let input = ctx.input();

    // --- Open a window app ---------------------------------------------------------
    if let Some(app) = open_app {
        if let Some(win) = windows.iter_mut().find(|w| w.app_name == app.name) {
            win.z = state.next_z;
            state.next_z += 1;
        } else {
            for req in app.requires {
                if !loaded(req) {
                    let _ = mm.reducers.load_app(req.to_string());
                    toast(&ctx, format!("{} needs {req}: starting it", app.title), true)?;
                }
            }
            let sid = state.next_surface_id;
            state.next_surface_id += 1;
            let offset = ((sid - 1) % 8) as f32 * 28.0;
            let wx = 120.0 + offset;
            let wy = TOPBAR_H + 40.0 + offset;
            let ww = 960.0_f32.min(sw - wx - 40.0).max(MIN_W);
            let wh = 640.0_f32.min(sh - wy - 40.0).max(MIN_H);
            let (cw, ch) = content_px(ww, wh);
            ctx.log(&format!("desktop: opening {} in window {sid}", app.name));
            let _ = mm.reducers.load_app(app.name.to_string());
            let _ = g.reducers.create_surface(sid, cw, ch);
            let _ = g.reducers.assign_module_surface(app.name.to_string(), sid);
            let _ = g.reducers.create_layer(win_content_layer(sid), WIN_Z_BASE, false);
            let _ = g.reducers.create_layer(win_chrome_layer(sid), WIN_Z_BASE + 1, false);
            windows.push(Window {
                id: sid,
                app_name: app.name.to_string(),
                surface_id: sid,
                title: format!("{} — {}", app.title, app.blurb),
                z: state.next_z,
                x: wx,
                y: wy,
                w: ww,
                h: wh,
                surf_w: cw,
                surf_h: ch,
                recording: false,
            });
            state.next_z += 1;
        }
        state.show_launcher = false;
    }

    // --- Start / stop a service ----------------------------------------------------
    if let Some(app) = toggle_service {
        if loaded(app.name) {
            let users: Vec<&str> = APPS
                .iter()
                .filter(|a| a.requires.contains(&app.name) && loaded(a.name))
                .map(|a| a.title)
                .collect();
            if users.is_empty() {
                let _ = mm.reducers.unload_app(app.name.to_string());
                toast(&ctx, format!("Stopped {}", app.name), true)?;
            } else {
                toast(&ctx, format!("{} is used by {}: close it first", app.name, users.join(", ")), false)?;
            }
        } else {
            let _ = mm.reducers.load_app(app.name.to_string());
            toast(&ctx, format!("Started {}", app.name), true)?;
        }
    }

    // --- Record / replay (the ● button) --------------------------------------------
    if let Some(id) = record_toggle {
        if let Some(win) = windows.iter_mut().find(|w| w.id == id) {
            if win.recording {
                // Replay what was recorded on a detached copy; the outcome
                // comes back as a ReplayReport (see `on_replay_report`).
                let _ = mm.reducers.replay(win.app_name.clone());
                let _ = mm.reducers.record_stop(win.app_name.clone());
                toast(&ctx, format!("Replaying {}…", win.app_name), true)?;
            } else {
                let _ = mm.reducers.record_start(win.app_name.clone());
                toast(&ctx, format!("Recording {}: click ● again to replay it", win.app_name), true)?;
            }
            win.recording = !win.recording;
        }
    }

    // --- Close a window ------------------------------------------------------------
    if let Some(id) = closed_id {
        if let Some(pos) = windows.iter().position(|w| w.id == id) {
            let win = windows.remove(pos);
            ctx.log(&format!("desktop: closing {} (window {})", win.app_name, win.id));
            let _ = mm.reducers.unload_app(win.app_name.clone());
            release_window(&ctx, &win);
        }
        interaction.mode = 0;
    }

    // --- Surfaces follow their windows' size -----------------------------------------
    for win in windows.iter_mut() {
        let (cw, ch) = content_px(win.w, win.h);
        if (win.surf_w, win.surf_h) != (cw, ch) {
            let _ = g.reducers.resize_surface(win.surface_id, cw, ch);
            win.surf_w = cw;
            win.surf_h = ch;
        }
    }

    // --- Input routing: routes, keyboard focus, pointer capture ----------------------
    for win in &windows {
        let old = before.iter().find(|b| b.id == win.id);
        if old.is_none_or(|o| (o.x, o.y, o.w, o.h, o.z) != (win.x, win.y, win.w, win.h, win.z)) {
            let (x, y, w, h) = content_rect(win);
            let _ = input.reducers.set_input_route(win.app_name.clone(), x, y, w, h, win.z);
        }
    }
    // The keyboard goes to the top window, or to nobody while the launcher is up.
    let focus = match state.show_launcher {
        true => String::new(),
        false => windows.iter().max_by_key(|w| w.z).map(|w| w.app_name.clone()).unwrap_or_default(),
    };
    if focus != state.focus {
        let _ = input.reducers.set_keyboard_focus(focus.clone());
        state.focus = focus;
    }
    if state.show_launcher != state.captured {
        let _ = input.reducers.set_pointer_capture(state.show_launcher);
        state.captured = state.show_launcher;
    }

    // --- Draw ----------------------------------------------------------------------------
    let now = host_calls::time_now_ms().unwrap_or(0);
    let toast_row = ctx.current.tables.toast().get(0)?.filter(|t| t.until_ms > now);
    draw_background(&ctx, sw, sh);
    let mut by_z: Vec<&Window> = windows.iter().collect();
    by_z.sort_by_key(|w| w.z);
    let top = by_z.last().map(|w| w.id);
    for (rank, win) in by_z.iter().enumerate() {
        draw_window(&ctx, win, rank, Some(win.id) == top && !state.show_launcher);
    }
    draw_topbar(&ctx, sw, &state, toast_row.as_ref(), now);
    if state.show_launcher {
        draw_launcher(&ctx, sw, sh, &apps);
    }
    let _ = g.reducers.draw_circle(CURSOR_LAYER.to_string(), Vec2 { x: mx, y: my }, 7.0, rgba(0.0, 0.0, 0.0, 0.5), true, 0.0);
    let _ = g.reducers.draw_circle(CURSOR_LAYER.to_string(), Vec2 { x: mx, y: my }, 6.0, rgba(1.0, 1.0, 1.0, 1.0), true, 0.0);

    // --- Write back what changed ----------------------------------------------------------
    let t = &ctx.current.tables;
    for win in &windows {
        match before.iter().find(|b| b.id == win.id) {
            None => t.window().insert(win.clone()).map(|_| ())?,
            Some(old) if old != win => t.window().update(win.clone())?,
            Some(_) => {}
        }
    }
    if let Some(id) = closed_id {
        t.window().delete(id)?;
    }
    if state != state0 {
        t.desktopstate().update(state)?;
    }
    if interaction != interaction0 {
        t.interaction().update(interaction)?;
    }
    if prev_down != left_down || prev_mouse.is_none() {
        let row = PrevMouse { id: 0, left_down };
        match prev_mouse {
            Some(_) => t.prevmouse().update(row)?,
            None => t.prevmouse().insert(row).map(|_| ())?,
        }
    }
    Ok(())
}

/// Open an app as if its launcher tile were clicked (scripting, tests):
/// `interstice call_reducer desktop-example desktop-example open notes`.
#[reducer]
pub fn open<Caps>(ctx: ReducerContext<Caps>, app: String) -> Result<(), String>
where
    Caps: CanRead<DesktopState> + CanUpdate<DesktopState>,
{
    let Some(mut state) = ctx.current.tables.desktopstate().get(0)? else {
        return Err("desktop: not loaded".into());
    };
    state.pending_open = app;
    ctx.current.tables.desktopstate().update(state)
}

/// Unload every app this desktop started.
#[reducer]
pub fn unload(ctx: ReducerContext) {
    ctx.log("desktop: unloading open apps");
    let mm = ctx.module_manager();
    for app in list_apps_sorted(&ctx).into_iter().filter(|a| a.loaded) {
        let _ = mm.reducers.unload_app(app.name);
    }
}

fn toast<Caps>(ctx: &ReducerContext<Caps>, text: String, ok: bool) -> Result<(), String>
where
    Caps: CanRead<Toast> + CanInsert<Toast> + CanUpdate<Toast>,
{
    let until_ms = host_calls::time_now_ms().unwrap_or(0) + TOAST_MS;
    let row = Toast { id: 0, text, ok, until_ms };
    if ctx.current.tables.toast().get(0)?.is_some() {
        ctx.current.tables.toast().update(row)
    } else {
        ctx.current.tables.toast().insert(row).map(|_| ())
    }
}

// ── Geometry ──────────────────────────────────────────────────────────────────

fn content_rect(win: &Window) -> (f32, f32, f32, f32) {
    (win.x, win.y + TITLEBAR_H, win.w, win.h - TITLEBAR_H)
}

fn content_px(w: f32, h: f32) -> (u32, u32) {
    (w.max(1.0) as u32, (h - TITLEBAR_H).max(1.0) as u32)
}

fn close_box(win: &Window) -> (f32, f32) {
    (win.x + win.w - TITLEBAR_H, win.y)
}

fn rec_box(win: &Window) -> (f32, f32) {
    (win.x + win.w - 2.0 * TITLEBAR_H, win.y)
}

fn in_rect(px: f32, py: f32, x: f32, y: f32, w: f32, h: f32) -> bool {
    px >= x && px <= x + w && py >= y && py <= y + h
}

/// The launcher grid: one tile per catalog app, centered below the top bar.
fn launcher_tiles(sw: f32) -> Vec<(&'static AppSpec, Rect)> {
    const TILE_W: f32 = 170.0;
    const TILE_H: f32 = 150.0;
    const GAP: f32 = 24.0;
    const MARGIN: f32 = 40.0;
    let usable = (sw - 2.0 * MARGIN).max(TILE_W);
    let cols = (((usable + GAP) / (TILE_W + GAP)).floor() as usize).clamp(1, 4);
    let grid_w = cols as f32 * TILE_W + (cols as f32 - 1.0) * GAP;
    let start_x = ((sw - grid_w) / 2.0).max(MARGIN);
    let start_y = TOPBAR_H + 90.0;
    APPS.iter()
        .enumerate()
        .map(|(i, app)| {
            let (col, row) = (i % cols, i / cols);
            let rect = Rect {
                x: start_x + col as f32 * (TILE_W + GAP),
                y: start_y + row as f32 * (TILE_H + GAP),
                w: TILE_W,
                h: TILE_H,
            };
            (app, rect)
        })
        .collect()
}

// ── Drawing ───────────────────────────────────────────────────────────────────

fn rgba(r: f32, g: f32, b: f32, a: f32) -> Color {
    Color { r, g, b, a }
}

const TEXT: (f32, f32, f32) = (0.92, 0.94, 0.98);

fn text<Caps>(ctx: &ReducerContext<Caps>, layer: &str, s: impl Into<String>, x: f32, y: f32, size: f32, c: Color) {
    let _ = ctx.graphics().reducers.draw_text(layer.to_string(), s.into(), Vec2 { x, y }, size, c, None);
}

fn rect<Caps>(ctx: &ReducerContext<Caps>, layer: &str, r: Rect, c: Color, radius: f32) {
    let radius = if radius > 0.0 { Some(radius) } else { None };
    let _ = ctx.graphics().reducers.draw_rect(layer.to_string(), r, c, true, 0.0, radius);
}

fn draw_background<Caps>(ctx: &ReducerContext<Caps>, sw: f32, sh: f32) {
    rect(ctx, DESKTOP_LAYER, Rect { x: 0.0, y: 0.0, w: sw, h: sh }, rgba(0.07, 0.08, 0.12, 1.0), 0.0);
    let c = rgba(0.30, 0.33, 0.42, 1.0);
    text(ctx, DESKTOP_LAYER, "Interstice desktop", 40.0, sh - 92.0, 26.0, rgba(0.30, 0.33, 0.42, 1.0));
    text(
        ctx,
        DESKTOP_LAYER,
        "Every app is a module: composited surfaces, routed input, loaded at runtime, recorded and replayed.",
        40.0,
        sh - 56.0,
        14.0,
        c,
    );
}

fn draw_window<Caps>(ctx: &ReducerContext<Caps>, win: &Window, rank: usize, focused: bool) {
    let g = ctx.graphics();
    let content_layer = win_content_layer(win.surface_id);
    let chrome = win_chrome_layer(win.surface_id);
    let base_z = WIN_Z_BASE + rank as i32 * 2;
    let _ = g.reducers.set_layer_z(content_layer.clone(), base_z);
    let _ = g.reducers.set_layer_z(chrome.clone(), base_z + 1);

    rect(ctx, &content_layer, Rect { x: win.x, y: win.y, w: win.w, h: win.h }, rgba(0.15, 0.16, 0.20, 1.0), 8.0);
    let (cx, cy, cw, ch) = content_rect(win);
    let _ = g.reducers.draw_surface(content_layer, win.surface_id, Rect { x: cx, y: cy, w: cw, h: ch }, rgba(1.0, 1.0, 1.0, 1.0));

    let bar = if focused { rgba(0.24, 0.28, 0.40, 1.0) } else { rgba(0.19, 0.20, 0.26, 1.0) };
    rect(ctx, &chrome, Rect { x: win.x, y: win.y, w: win.w, h: TITLEBAR_H }, bar, 8.0);
    text(ctx, &chrome, win.title.clone(), win.x + 12.0, win.y + 8.0, 15.0, rgba(TEXT.0, TEXT.1, TEXT.2, 1.0));

    // ● record / replay.
    let (rx, ry) = rec_box(win);
    let centre = Vec2 { x: rx + TITLEBAR_H / 2.0, y: ry + TITLEBAR_H / 2.0 };
    let rec = if win.recording { rgba(0.95, 0.25, 0.25, 1.0) } else { rgba(0.55, 0.30, 0.32, 1.0) };
    let _ = g.reducers.draw_circle(chrome.clone(), centre, 7.0, rec, win.recording, 2.0);
    if win.recording {
        text(ctx, &chrome, "REC", rx - 30.0, ry + 9.0, 12.0, rgba(0.95, 0.35, 0.35, 1.0));
    }
    // Close box.
    let (kx, ky) = close_box(win);
    rect(
        ctx,
        &chrome,
        Rect { x: kx + 6.0, y: ky + 6.0, w: TITLEBAR_H - 12.0, h: TITLEBAR_H - 12.0 },
        rgba(0.80, 0.30, 0.32, 1.0),
        4.0,
    );
    // Resize grip.
    rect(
        ctx,
        &chrome,
        Rect { x: win.x + win.w - RESIZE_HANDLE, y: win.y + win.h - RESIZE_HANDLE, w: RESIZE_HANDLE, h: RESIZE_HANDLE },
        rgba(0.45, 0.48, 0.58, 0.9),
        3.0,
    );
    let border = if focused { rgba(0.45, 0.55, 0.85, 1.0) } else { rgba(0.35, 0.38, 0.48, 1.0) };
    let _ = g.reducers.draw_rect(chrome, Rect { x: win.x, y: win.y, w: win.w, h: win.h }, border, false, 2.0, Some(8.0));
}

fn draw_topbar<Caps>(ctx: &ReducerContext<Caps>, sw: f32, state: &DesktopState, toast: Option<&Toast>, now_ms: u64) {
    let l = LAUNCHER_LAYER;
    rect(ctx, l, Rect { x: 0.0, y: 0.0, w: sw, h: TOPBAR_H }, rgba(0.10, 0.11, 0.16, 0.95), 0.0);
    let apps_bg = if state.show_launcher { rgba(0.30, 0.44, 0.78, 1.0) } else { rgba(0.20, 0.22, 0.30, 1.0) };
    rect(ctx, l, Rect { x: 8.0, y: 4.0, w: 80.0, h: TOPBAR_H - 8.0 }, apps_bg, 6.0);
    text(ctx, l, "Apps", 30.0, 11.0, 14.0, rgba(TEXT.0, TEXT.1, TEXT.2, 1.0));
    if let Some(t) = toast {
        let c = if t.ok { rgba(0.55, 0.85, 0.60, 1.0) } else { rgba(0.95, 0.55, 0.50, 1.0) };
        let mut s = t.text.clone();
        let max_chars = ((sw - 240.0) / 7.5).max(10.0) as usize;
        if s.chars().count() > max_chars {
            s = s.chars().take(max_chars.saturating_sub(1)).collect::<String>() + "…";
        }
        text(ctx, l, s, 104.0, 11.0, 14.0, c);
    }
    // Clock (UTC), from the transaction clock.
    let secs = now_ms / 1000;
    let clock = format!("{:02}:{:02}:{:02} UTC", (secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    text(ctx, l, clock, sw - 110.0, 11.0, 14.0, rgba(0.70, 0.72, 0.80, 1.0));
}

fn draw_launcher<Caps>(ctx: &ReducerContext<Caps>, sw: f32, sh: f32, apps: &[AppInfo]) {
    let l = LAUNCHER_LAYER;
    rect(ctx, l, Rect { x: 0.0, y: TOPBAR_H, w: sw, h: sh - TOPBAR_H }, rgba(0.04, 0.05, 0.08, 0.86), 0.0);
    let tiles = launcher_tiles(sw);
    let x0 = tiles.first().map(|(_, r)| r.x).unwrap_or(40.0);
    text(ctx, l, "Apps", x0, TOPBAR_H + 24.0, 22.0, rgba(TEXT.0, TEXT.1, TEXT.2, 1.0));
    text(
        ctx,
        l,
        "Click an app to open its window, a service to start or stop it. In a window, ● records the app; click it again to replay.",
        x0,
        TOPBAR_H + 56.0,
        13.0,
        rgba(0.65, 0.67, 0.75, 1.0),
    );
    for (app, r) in tiles {
        let running = apps.iter().any(|a| a.name == app.name && a.loaded);
        rect(ctx, l, Rect { x: r.x, y: r.y, w: r.w, h: r.h }, rgba(0.16, 0.18, 0.24, 0.95), 10.0);
        let icon = 56.0;
        let _ = ctx.graphics().reducers.draw_image(
            l.to_string(),
            icon_texture_id(app.name),
            Rect { x: r.x + (r.w - icon) / 2.0, y: r.y + 12.0, w: icon, h: icon },
            rgba(1.0, 1.0, 1.0, 1.0),
            Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 },
        );
        text(ctx, l, app.title, r.x + 12.0, r.y + r.h - 50.0, 15.0, rgba(TEXT.0, TEXT.1, TEXT.2, 1.0));
        text(ctx, l, app.blurb, r.x + 12.0, r.y + r.h - 26.0, 12.0, rgba(0.62, 0.64, 0.72, 1.0));
        if app.kind == Kind::Service {
            let label = if running { "running" } else { "stopped" };
            text(ctx, l, label, r.x + r.w - 62.0, r.y + 10.0, 11.0, rgba(0.62, 0.64, 0.72, 1.0));
        }
        if running {
            let _ = ctx.graphics().reducers.draw_circle(
                l.to_string(),
                Vec2 { x: r.x + 14.0, y: r.y + 16.0 },
                5.0,
                rgba(0.30, 0.80, 0.40, 1.0),
                true,
                0.0,
            );
        }
    }
}

// ── Apps ──────────────────────────────────────────────────────────────────────

fn icon_texture_id(app_name: &str) -> String {
    format!("icon_{app_name}")
}

fn win_content_layer(id: u32) -> String {
    format!("win_content_{id}")
}

fn win_chrome_layer(id: u32) -> String {
    format!("win_chrome_{id}")
}

fn list_apps_sorted<Caps>(ctx: &ReducerContext<Caps>) -> Vec<AppInfo> {
    let mut apps = ctx.module_manager().queries.list_apps().unwrap_or_default();
    apps.sort_by_key(|a| a.id);
    apps
}

/// Register an app (without loading it) and upload its launcher icon. The
/// wasm goes as a call-scoped Blob; module_manager skips the rewrite when the
/// bytes are unchanged.
fn register_app_with_icon<Caps>(ctx: &ReducerContext<Caps>, name: &str, bytes: &[u8], icon: Vec<u8>) {
    let _ = ctx.module_manager().reducers.register_app(name.to_string(), Blob::new(bytes), Some(icon.clone()));
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
        Blob::new(&icon),
    );
}

/// An app icon as raw RGBA8: a vertical gradient from the app's colour with a
/// white disc in the middle.
fn make_icon(base: (f32, f32, f32)) -> Vec<u8> {
    let n = ICON_PX as usize;
    let mut data = vec![0u8; n * n * 4];
    let c = (n as f32 - 1.0) / 2.0;
    for y in 0..n {
        for x in 0..n {
            let idx = (y * n + x) * 4;
            let t = y as f32 / n as f32;
            let (dx, dy) = (x as f32 - c, y as f32 - c);
            let d = (dx * dx + dy * dy).sqrt();
            let px = if d < 18.0 {
                [240, 244, 250]
            } else if d < 22.0 {
                [120, 140, 200]
            } else {
                [(base.0 + 40.0 * t) as u8, (base.1 + 60.0 * t) as u8, (base.2 + 60.0 * t) as u8]
            };
            data[idx..idx + 3].copy_from_slice(&px);
            data[idx + 3] = 255;
        }
    }
    data
}
