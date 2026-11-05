use crate::bindings::{graphics::*, input::*, module_manager::*};
use interstice_sdk::*;
use interstice_ui::FontStyle;
use interstice_ui::theme::{self as th, Canvas, Image, Rgba};

interstice_module!(visibility: Public);

// The desktop: an OS made of modules (DESIGN §12.2). It holds no authority.
// It is the graphics *compositor* (each app renders into its own offscreen
// surface, composited here into windows), the input *router* (each app gets
// the pointer in its own coordinates and the keyboard only when focused), and
// a client of `module_manager` (apps are registered wasm blobs, loaded and
// unloaded at runtime).
//
// Its look follows brand/BRAND.md: everything is drawn from
// `interstice_ui::theme`; text that never changes (the wordmark, labels) is
// baked into textures at load, because the renderer has no letter-spacing
// and no blur.
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
    /// Draws the app's monochrome glyph on a 0..1 canvas.
    glyph: fn(&mut Canvas),
    bytes: &'static [u8],
}

const APPS: &[AppSpec] = &[
    AppSpec {
        name: "ui-example",
        title: "UI demo",
        blurb: "Layout, text and inputs",
        kind: Kind::Window,
        requires: &[],
        glyph: glyph_ui,
        bytes: example_wasm!("ui_example.wasm"),
    },
    AppSpec {
        name: "notes",
        title: "Notes",
        blurb: "Time travel on a logged table",
        kind: Kind::Window,
        requires: &["hello-example"],
        glyph: glyph_notes,
        bytes: example_wasm!("notes.wasm"),
    },
    AppSpec {
        name: "browser-example",
        title: "Browser",
        blurb: "Blitz, JavaScript, network",
        kind: Kind::Window,
        requires: &[],
        glyph: glyph_browser,
        bytes: example_wasm!("browser_example.wasm"),
    },
    AppSpec {
        name: "agar-client",
        title: "Agar",
        blurb: "Multiplayer; needs agar-server",
        kind: Kind::Window,
        requires: &[],
        glyph: glyph_agar,
        bytes: example_wasm!("agar_client.wasm"),
    },
    AppSpec {
        name: "hello-example",
        title: "Hello",
        blurb: "The notes backend",
        kind: Kind::Service,
        requires: &[],
        glyph: glyph_table,
        bytes: example_wasm!("hello_example.wasm"),
    },
    AppSpec {
        name: "http-get-example",
        title: "HTTP GET",
        blurb: "Network broker",
        kind: Kind::Service,
        requires: &[],
        glyph: glyph_http,
        bytes: example_wasm!("http_get_example.wasm"),
    },
    AppSpec {
        name: "audio-example",
        title: "Audio",
        blurb: "A 440 Hz tone",
        kind: Kind::Service,
        requires: &[],
        glyph: glyph_audio,
        bytes: example_wasm!("audio_example.wasm"),
    },
];

/// Size the app glyphs are rasterized at (drawn smaller, linearly filtered).
const ICON_PX: u32 = 96;

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
const TITLEBAR_H: f32 = 34.0;
/// The app surface is inset this much from the window's sides and bottom, so
/// its square corners stay inside the window's rounded ones.
const INSET: f32 = 3.0;
const RESIZE_HANDLE: f32 = 18.0;
const MIN_W: f32 = 240.0;
const MIN_H: f32 = 160.0;
const TOAST_MS: u64 = 4000;
/// Open / launcher fade-in, and toast fade (BRAND.md §6).
const OPEN_MS: u64 = 180;
const TOAST_IN_MS: u64 = 220;
const TOAST_OUT_MS: u64 = 180;
/// Breathing period of the slit while something starts.
const BREATH_MS: u64 = 4000;

/// Wordmark letter sizes: the wallpaper's hero and the top bar's.
const HERO_PX: f32 = 44.0;
const BAR_MARK_PX: f32 = 11.0;
const PILLARS: &str = "MODULAR  /  OPEN  /  EXTENSIBLE  /  FORWARD-LOOKING";
/// The uppercase labels, baked white and tinted when drawn.
const LABELS: &[&str] = &["APPS", "APPLICATIONS", "SERVICES", "RUNNING", "STOPPED", "STARTING"];

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
    /// When the window opened, for its fade-in.
    opened_ms: u64,
}

/// Singleton desktop bookkeeping. `focus` and `captured` mirror what was last
/// sent to `input`, so the routing reducers are called only on change.
#[table]
#[derive(PartialEq)]
pub struct DesktopState {
    #[primary_key]
    id: u32,
    show_launcher: bool,
    /// When the launcher last opened, for its fade-in.
    launcher_ms: u64,
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

/// Sizes of the baked textures, to lay them out (the textures themselves
/// live in graphics).
#[table]
pub struct Baked {
    #[primary_key]
    id: String,
    w: f32,
    h: f32,
    /// Margin around the drawn box (the wordmark's glow spills past it).
    pad: f32,
}

#[reducer(on = "load")]
pub fn on_load<Caps>(ctx: ReducerContext<Caps>)
where
    Caps: CanInsert<DesktopState> + CanInsert<Interaction> + CanInsert<PrevMouse> + CanInsert<Baked>,
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
        let mut c = Canvas::new(ICON_PX);
        (app.glyph)(&mut c);
        let glyph = Image { width: ICON_PX, height: ICON_PX, rgba: c.to_rgba((1.0, 1.0, 1.0, 1.0), (0.0, 0.0, 0.0, 0.0)) };
        let tile = c.to_rgba(th::IVORY, th::RAISED);
        let _ = ctx.module_manager().reducers.register_app(app.name.to_string(), Blob::new(app.bytes), Some(tile));
        upload(&ctx, &icon_texture_id(app.name), &glyph);
    }
    bake_brand(&ctx);

    let _ = ctx.current.tables.desktopstate().insert(DesktopState {
        id: 0,
        show_launcher: true,
        launcher_ms: 0,
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

/// Rasterize the textures that never change: the wordmarks, the tagline and
/// pillars, the labels, the slit, the cursor.
fn bake_brand<Caps>(ctx: &ReducerContext<Caps>)
where
    Caps: CanInsert<Baked>,
{
    let white = (1.0, 1.0, 1.0, 1.0);
    let put = |id: &str, img: &Image, pad: f32| {
        upload(ctx, id, img);
        let row = Baked { id: id.to_string(), w: img.width as f32, h: img.height as f32, pad };
        let _ = ctx.current.tables.baked().insert(row);
    };
    let (hero, pad) = th::wordmark_image(HERO_PX, th::IVORY);
    put("hero", &hero, pad);
    let (mark, pad) = th::wordmark_image(BAR_MARK_PX, th::IVORY);
    put("barmark", &mark, pad);
    put("tagline", &th::text_image(th::TAGLINE, 13.0, th::TAGLINE_TRACKING, FontStyle::REGULAR, white), 0.0);
    put("pillars", &th::text_image(PILLARS, th::TEXT_CAPTION, th::LABEL_TRACKING, FontStyle::REGULAR, white), 0.0);
    for l in LABELS {
        let img = th::text_image(l, th::TEXT_CAPTION, th::LABEL_TRACKING, FontStyle::REGULAR, white);
        put(&label_id(l), &img, 0.0);
    }
    put("slit", &Image { width: 64, height: 256, rgba: th::slit_rgba(64, 256, 1.0) }, 0.0);
    // The title bar's slit, baked at the size it's drawn so its core stays a crisp hairline.
    put("slit_small", &Image { width: 11, height: 26, rgba: th::slit_rgba(11, 26, 1.3) }, 0.0);
    put("cursor", &cursor_image(), 0.0);
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
        let title = APPS.iter().find(|a| a.name == report.module).map_or(report.module.as_str(), |a| a.title);
        return toast(&ctx, format!("{title} is running"), true);
    }
    let windows = ctx.current.tables.window().scan()?;
    if let Some(win) = windows.iter().find(|w| !report.module.is_empty() && w.app_name == report.module) {
        ctx.current.tables.window().delete(win.id)?;
        release_window(&ctx, win);
    }
    let first = report.message.lines().next().unwrap_or("").to_string();
    toast(&ctx, first, false)
}

/// Give back what a window holds outside the desktop's tables: its input
/// route, surface and layers. The caller unloads the app
/// (unless the load already failed) and deletes the `Window` row.
fn release_window<Caps>(ctx: &ReducerContext<Caps>, win: &Window) {
    let g = ctx.graphics();
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
        + CanUpdate<Toast>
        + CanRead<Baked>,
{
    // We are unassigned, so this is the swapchain: the real screen size.
    let Ok(info) = ctx.graphics().queries.surface_info() else {
        return Ok(());
    };
    let (sw, sh) = (info.width as f32, info.height as f32);
    if sw < 1.0 || sh < 1.0 {
        return Ok(());
    }
    let now = host_calls::time_now_ms().unwrap_or(0);

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
    if pressed_edge {
        if apps_button().contains(mx, my) {
            state.show_launcher = !state.show_launcher;
            if state.show_launcher {
                state.launcher_ms = now;
            }
        } else if state.show_launcher && my > TOPBAR_H {
            let hit = launcher_layout(sw).tiles.into_iter().find(|(_, r)| r.contains(mx, my));
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
            if let Some(i) = order.into_iter().find(|&i| windows[i].outer().contains(mx, my)) {
                let win = &mut windows[i];
                win.z = state.next_z;
                state.next_z += 1;
                let on_titlebar = my < win.y + TITLEBAR_H;
                if on_titlebar && close_box(win).contains(mx, my) {
                    closed_id = Some(win.id);
                } else if resize_handle(win).contains(mx, my) {
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
                title: app.title.to_string(),
                z: state.next_z,
                x: wx,
                y: wy,
                w: ww,
                h: wh,
                surf_w: cw,
                surf_h: ch,
                opened_ms: now,
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
                toast(&ctx, format!("Stopped {}", app.title), true)?;
            } else {
                toast(&ctx, format!("{} is used by {}: close it first", app.title, users.join(", ")), false)?;
            }
        } else {
            let _ = mm.reducers.load_app(app.name.to_string());
            toast(&ctx, format!("Starting {}", app.title), true)?;
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
            let c = content_rect(win);
            let _ = input.reducers.set_input_route(win.app_name.clone(), c.x, c.y, c.w, c.h, win.z);
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
    let toast_row = ctx.current.tables.toast().get(0)?.filter(|t| t.until_ms > now);
    let d = Draw { ctx: &ctx, now, mouse: (mx, my) };
    // The hero is hidden once the launcher's scrim is fully over it.
    let launcher_settled = state.show_launcher && ease(now, state.launcher_ms, OPEN_MS) >= 1.0;
    d.background(sw, sh, !launcher_settled);
    let mut by_z: Vec<&Window> = windows.iter().collect();
    by_z.sort_by_key(|w| w.z);
    let top = by_z.last().map(|w| w.id);
    // Hover goes to the top-most window under the pointer only.
    let hovered = by_z.iter().rev().find(|w| w.outer().contains(mx, my)).map(|w| w.id);
    for (rank, win) in by_z.iter().enumerate() {
        let focused = Some(win.id) == top && !state.show_launcher;
        let hover = Some(win.id) == hovered && !state.show_launcher;
        d.window(win, rank, focused, hover, loaded(&win.app_name));
    }
    d.topbar(sw, &state, &apps, toast_row.as_ref());
    if state.show_launcher {
        d.launcher(sw, sh, &apps, state.launcher_ms);
    }
    d.image(CURSOR_LAYER, "cursor", rect(mx - 2.4, my - 1.4, 24.0, 24.0), (1.0, 1.0, 1.0, 1.0));

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

trait RectExt {
    fn contains(&self, px: f32, py: f32) -> bool;
}

impl RectExt for Rect {
    fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
}

fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect { x, y, w, h }
}

/// Bindings types aren't `Clone`.
fn cp(r: &Rect) -> Rect {
    rect(r.x, r.y, r.w, r.h)
}

impl Window {
    fn outer(&self) -> Rect {
        rect(self.x, self.y, self.w, self.h)
    }
}

/// The app's surface: below the title bar, inset from the other three sides.
fn content_rect(win: &Window) -> Rect {
    rect(win.x + INSET, win.y + TITLEBAR_H, win.w - 2.0 * INSET, win.h - TITLEBAR_H - INSET)
}

fn content_px(w: f32, h: f32) -> (u32, u32) {
    ((w - 2.0 * INSET).max(1.0) as u32, (h - TITLEBAR_H - INSET).max(1.0) as u32)
}

fn close_box(win: &Window) -> Rect {
    rect(win.x + win.w - TITLEBAR_H - 4.0, win.y, TITLEBAR_H, TITLEBAR_H)
}

fn resize_handle(win: &Window) -> Rect {
    rect(win.x + win.w - RESIZE_HANDLE, win.y + win.h - RESIZE_HANDLE, RESIZE_HANDLE, RESIZE_HANDLE)
}

/// The top bar's `APPS` button (the wordmark is part of it).
fn apps_button() -> Rect {
    rect(0.0, 0.0, apps_label_x() + label_width("APPS") + 14.0, TOPBAR_H)
}

fn apps_label_x() -> f32 {
    (16.0 + th::wordmark(BAR_MARK_PX).width + 28.0).round()
}

fn label_width(s: &str) -> f32 {
    th::tracked(s, th::TEXT_CAPTION, th::LABEL_TRACKING, FontStyle::REGULAR).1
}

const TILE_W: f32 = 216.0;
const TILE_H: f32 = 124.0;
const TILE_GAP: f32 = 16.0;

struct LauncherLayout {
    x0: f32,
    width: f32,
    /// Where each section's label sits: (label, y).
    sections: Vec<(&'static str, f32)>,
    tiles: Vec<(&'static AppSpec, Rect)>,
}

/// The launcher: an `Apps` heading, then a section of window apps and one of
/// services, each a grid of tiles under a hairline label.
fn launcher_layout(sw: f32) -> LauncherLayout {
    const MARGIN: f32 = 48.0;
    let usable = (sw - 2.0 * MARGIN).max(TILE_W);
    let cols = (((usable + TILE_GAP) / (TILE_W + TILE_GAP)).floor() as usize).clamp(1, 4);
    let width = cols as f32 * TILE_W + (cols as f32 - 1.0) * TILE_GAP;
    let x0 = ((sw - width) / 2.0).max(MARGIN).round();
    let mut y = TOPBAR_H + 132.0;
    let mut sections = Vec::new();
    let mut tiles = Vec::new();
    for (label, kind) in [("APPLICATIONS", Kind::Window), ("SERVICES", Kind::Service)] {
        sections.push((label, y));
        y += 28.0;
        let group: Vec<&'static AppSpec> = APPS.iter().filter(|a| a.kind == kind).collect();
        for (i, app) in group.iter().enumerate() {
            let (col, row) = (i % cols, i / cols);
            let r = rect(x0 + col as f32 * (TILE_W + TILE_GAP), y + row as f32 * (TILE_H + TILE_GAP), TILE_W, TILE_H);
            tiles.push((*app, r));
        }
        let rows = group.len().div_ceil(cols);
        y += rows as f32 * (TILE_H + TILE_GAP) + 24.0;
    }
    LauncherLayout { x0, width, sections, tiles }
}

// ── Drawing ───────────────────────────────────────────────────────────────────

fn col(c: Rgba) -> Color {
    Color { r: c.0, g: c.1, b: c.2, a: c.3 }
}

/// Ease-out progress of an animation that started at `since`.
fn ease(now: u64, since: u64, dur: u64) -> f32 {
    let t = (now.saturating_sub(since) as f32 / dur as f32).clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Slit breathing (BRAND.md §2): opacity 1 → 0.65 → 1 over 4 s.
fn breath(now: u64) -> f32 {
    let phase = (now % BREATH_MS) as f32 / BREATH_MS as f32 * std::f32::consts::TAU;
    0.825 + 0.175 * phase.cos()
}

/// Truncate `s` with an ellipsis to fit `max_w` at `size`.
fn fit(s: &str, size: f32, style: FontStyle, max_w: f32) -> String {
    if interstice_ui::text_width(s, size, style) <= max_w {
        return s.to_string();
    }
    let mut out = String::new();
    for ch in s.chars() {
        if interstice_ui::text_width(&format!("{out}{ch}…"), size, style) > max_w {
            break;
        }
        out.push(ch);
    }
    out + "…"
}

/// One frame's drawing: the reducer context plus what every part needs.
struct Draw<'a, Caps> {
    ctx: &'a ReducerContext<Caps>,
    now: u64,
    mouse: (f32, f32),
}

impl<Caps> Draw<'_, Caps>
where
    Caps: CanRead<Baked>,
{
    fn hover(&self, r: &Rect) -> bool {
        r.contains(self.mouse.0, self.mouse.1)
    }

    fn fill(&self, layer: &str, r: Rect, c: Rgba, radius: f32) {
        if r.w <= 0.0 || r.h <= 0.0 || c.3 <= 0.0 {
            return;
        }
        let radius = (radius > 0.0).then_some(radius);
        let _ = self.ctx.graphics().reducers.draw_rect(layer.to_string(), r, col(c), true, 0.0, radius);
    }

    fn stroke(&self, layer: &str, r: Rect, c: Rgba, radius: f32) {
        if c.3 <= 0.0 {
            return;
        }
        let radius = (radius > 0.0).then_some(radius);
        let _ = self.ctx.graphics().reducers.draw_rect(layer.to_string(), r, col(c), false, 1.0, radius);
    }

    /// A 1 px horizontal hairline.
    fn hline(&self, layer: &str, x: f32, y: f32, w: f32, c: Rgba) {
        self.fill(layer, rect(x, y, w, 1.0), c, 0.0);
    }

    fn dot(&self, layer: &str, x: f32, y: f32, r: f32, c: Rgba) {
        if c.3 <= 0.0 {
            return;
        }
        let _ = self.ctx.graphics().reducers.draw_circle(layer.to_string(), Vec2 { x, y }, r, col(c), true, 0.0);
    }

    fn text(&self, layer: &str, s: impl Into<String>, x: f32, y: f32, size: f32, c: Rgba, style: FontStyle) {
        if c.3 <= 0.0 {
            return;
        }
        let pos = Vec2 { x: x.round(), y: y.round() };
        let _ = self.ctx.graphics().reducers.draw_text(layer.to_string(), s.into(), pos, size, col(c), style.selector());
    }

    fn image(&self, layer: &str, id: &str, r: Rect, tint: Rgba) {
        if tint.3 <= 0.0 {
            return;
        }
        let uv = rect(0.0, 0.0, 1.0, 1.0);
        let _ = self.ctx.graphics().reducers.draw_image(layer.to_string(), id.to_string(), r, col(tint), uv);
    }

    fn baked(&self, id: &str) -> Option<Baked> {
        self.ctx.current.tables.baked().get(id.to_string()).ok().flatten()
    }

    /// A baked texture, drawn 1:1 with its box's top-left at (x, y).
    fn baked_at(&self, layer: &str, id: &str, x: f32, y: f32, tint: Rgba) -> Option<Baked> {
        let b = self.baked(id)?;
        self.image(layer, id, rect((x - b.pad).round(), (y - b.pad).round(), b.w, b.h), tint);
        Some(b)
    }

    /// An uppercase label (baked) at (x, y); returns its width.
    fn label(&self, layer: &str, s: &str, x: f32, y: f32, c: Rgba) -> f32 {
        self.baked_at(layer, &label_id(s), x, y, c).map_or(0.0, |b| b.w)
    }

    /// A section header in the board's style: label, then a hairline to `x_end`.
    fn section(&self, layer: &str, s: &str, x: f32, y: f32, x_end: f32, a: f32) {
        let w = self.label(layer, s, x, y, th::alpha(th::GRAY, a));
        let lx = x + w + 14.0;
        self.hline(layer, lx, (y + 8.0).round(), x_end - lx, th::alpha(th::LINE, a));
    }

    /// The wallpaper: the brand board's hero, centred, and its pillars along
    /// the bottom.
    fn background(&self, sw: f32, sh: f32, hero: bool) {
        let l = DESKTOP_LAYER;
        self.fill(l, rect(0.0, 0.0, sw, sh), th::GROUND, 0.0);
        if !hero {
            return;
        }
        let wm = th::wordmark(HERO_PX);
        let tag_w = self.baked("tagline").map_or(0.0, |t| t.w);
        let block_h = wm.height + 44.0;
        let top = ((TOPBAR_H + sh) / 2.0 - block_h / 2.0 - 16.0).round();
        self.baked_at(l, "hero", ((sw - wm.width) / 2.0).round(), top, (1.0, 1.0, 1.0, 0.94));
        self.baked_at(l, "tagline", ((sw - tag_w) / 2.0).round(), top + wm.height + 28.0, th::GRAY);

        let pw = self.baked("pillars").map_or(0.0, |p| p.w);
        let y = sh - 44.0;
        self.baked_at(l, "pillars", 40.0, y, th::alpha(th::GRAY, 0.85));
        self.hline(l, 40.0 + pw + 18.0, y + 7.0, sw - 80.0 - pw - 18.0, th::LINE);
    }

    fn window(&self, win: &Window, rank: usize, focused: bool, hover: bool, loaded: bool) {
        let g = self.ctx.graphics();
        let content = win_content_layer(win.surface_id);
        let chrome = win_chrome_layer(win.surface_id);
        let base_z = WIN_Z_BASE + rank as i32 * 2;
        let _ = g.reducers.set_layer_z(content.clone(), base_z);
        let _ = g.reducers.set_layer_z(chrome.clone(), base_z + 1);

        // Opening: fade in and rise 6 px.
        let a = ease(self.now, win.opened_ms, OPEN_MS);
        let dy = ((1.0 - a) * 6.0).round();
        let (x, y, w, h) = (win.x, win.y + dy, win.w, win.h);

        // Shadow: a few soft steps below the window.
        for (grow, alpha) in [(12.0, 0.04), (7.0, 0.07), (3.0, 0.10)] {
            let r = rect(x - grow, y - grow + 8.0, w + 2.0 * grow, h + 2.0 * grow);
            self.fill(&content, r, (0.0, 0.0, 0.0, alpha * a), th::RADIUS_WINDOW + grow);
        }
        self.fill(&content, rect(x, y, w, h), th::alpha(th::CHARCOAL, a), th::RADIUS_WINDOW);
        let mut c = content_rect(win);
        c.y += dy;
        let _ = g.reducers.draw_surface(content, win.surface_id, cp(&c), col((1.0, 1.0, 1.0, a)));

        // Still starting: the app hasn't drawn yet, so the window shows the slit, breathing.
        if !loaded {
            self.fill(&chrome, cp(&c), th::alpha(th::CHARCOAL, a), 0.0);
            let sh = (c.h * 0.32).clamp(40.0, 140.0);
            let sw = sh * 0.25;
            let (cx, cy) = (c.x + c.w / 2.0, c.y + c.h / 2.0 - 16.0);
            let r = rect(cx - sw / 2.0, cy - sh / 2.0, sw, sh);
            self.image(&chrome, "slit", r, (1.0, 1.0, 1.0, breath(self.now) * a));
            let lw = label_width("STARTING");
            self.label(&chrome, "STARTING", (cx - lw / 2.0).round(), cy + sh / 2.0 + 18.0, th::alpha(th::GRAY, a));
        }

        // Title bar: the body's own surface, a hairline below it.
        self.hline(&chrome, x + 1.0, y + TITLEBAR_H - 1.0, w - 2.0, th::alpha(th::LINE, a));
        // The slit before the title is lit only on the focused window.
        let slit_tint = if focused { (1.0, 1.0, 1.0, a) } else { th::alpha(th::DIM, 0.9 * a) };
        self.image(&chrome, "slit_small", rect(x + 10.0, y + 4.0, 11.0, 26.0), slit_tint);
        let tx = x + 28.0;
        let max_w = (w - 28.0 - TITLEBAR_H - 16.0).max(0.0);
        let title = fit(&win.title, th::TEXT_BODY, FontStyle::MEDIUM, max_w);
        let title_c = if focused { th::IVORY } else { th::MUTED };
        self.text(&chrome, title.clone(), tx, y + 9.0, th::TEXT_BODY, th::alpha(title_c, a), FontStyle::MEDIUM);
        let title_w = interstice_ui::text_width(&title, th::TEXT_BODY, FontStyle::MEDIUM);
        let rest = max_w - title_w - 12.0;
        let blurb = APPS.iter().find(|s| s.name == win.app_name).map_or("", |s| s.blurb);
        if rest > 40.0 && title == win.title {
            let b = fit(blurb, th::TEXT_BODY, FontStyle::REGULAR, rest);
            self.text(&chrome, b, tx + title_w + 12.0, y + 9.0, th::TEXT_BODY, th::alpha(th::GRAY, a), FontStyle::REGULAR);
        }

        // Close: a quiet ×, danger on hover.
        let mut k = close_box(win);
        k.y += dy;
        let over = hover && self.hover(&k);
        if over {
            let r = rect(k.x + 6.0, k.y + 6.0, k.w - 12.0, k.h - 12.0);
            self.fill(&chrome, r, th::alpha(th::DANGER, 0.14 * a), th::RADIUS_CONTROL);
        }
        let xc = if over { th::DANGER } else { th::GRAY };
        let xw = interstice_ui::text_width("×", 18.0, FontStyle::REGULAR);
        self.text(&chrome, "×", k.x + (k.w - xw) / 2.0, k.y + 6.0, 18.0, th::alpha(xc, a), FontStyle::REGULAR);

        // Resize grip: three dots on the diagonal.
        let (gx, gy) = (x + w - 8.0, y + h - 8.0);
        for (ox, oy) in [(0.0, 0.0), (-5.0, 0.0), (0.0, -5.0)] {
            self.dot(&chrome, gx + ox, gy + oy, 1.2, th::alpha(th::DIM, a));
        }

        let border = if focused { th::FOCUS_BORDER } else { th::WINDOW_BORDER };
        self.stroke(&chrome, rect(x, y, w, h), th::alpha(border, border.3 * a), th::RADIUS_WINDOW);
    }

    fn topbar(&self, sw: f32, state: &DesktopState, apps: &[AppInfo], toast: Option<&Toast>) {
        let l = LAUNCHER_LAYER;
        self.fill(l, rect(0.0, 0.0, sw, TOPBAR_H), th::CHARCOAL, 0.0);
        self.hline(l, 0.0, TOPBAR_H - 1.0, sw, th::TOPBAR_RULE);

        // The wordmark, then APPS: lit while the launcher is open.
        let wm = th::wordmark(BAR_MARK_PX);
        self.baked_at(l, "barmark", 16.0, ((TOPBAR_H - wm.height) / 2.0).round(), (1.0, 1.0, 1.0, 1.0));
        let ax = apps_label_x();
        let c = if state.show_launcher {
            th::AMBER
        } else if self.hover(&apps_button()) {
            th::IVORY
        } else {
            th::MUTED
        };
        self.label(l, "APPS", ax, 11.0, c);
        if state.show_launcher {
            self.fill(l, rect(ax, TOPBAR_H - 2.0, label_width("APPS"), 2.0), th::AMBER, 0.0);
        }

        // Right: running services, then the clock (UTC, from the transaction clock).
        let secs = self.now / 1000;
        let clock = format!("{:02}:{:02}:{:02}", (secs / 3600) % 24, (secs / 60) % 60, secs % 60);
        let cw = interstice_ui::text_width(&clock, th::TEXT_SMALL, FontStyle::MONO);
        let mut x = sw - 16.0 - cw;
        self.text(l, clock, x, 10.0, th::TEXT_SMALL, th::MUTED, FontStyle::MONO);
        x -= 24.0;
        for app in APPS.iter().rev().filter(|a| a.kind == Kind::Service) {
            if !apps.iter().any(|a| a.name == app.name && a.loaded) {
                continue;
            }
            x -= interstice_ui::text_width(app.title, th::TEXT_SMALL, FontStyle::REGULAR);
            self.text(l, app.title, x, 10.0, th::TEXT_SMALL, th::GRAY, FontStyle::REGULAR);
            self.dot(l, x - 10.0, TOPBAR_H / 2.0, 3.0, th::AMBER);
            x -= 30.0;
        }

        // The toast: a pill in the middle of the bar.
        if let Some(t) = toast {
            let start = t.until_ms.saturating_sub(TOAST_MS);
            let a_in = ease(self.now, start, TOAST_IN_MS);
            let a_out = (t.until_ms.saturating_sub(self.now) as f32 / TOAST_OUT_MS as f32).clamp(0.0, 1.0);
            let a = a_in.min(a_out);
            let s = fit(&t.text, th::TEXT_BODY, FontStyle::REGULAR, (sw * 0.4).max(160.0));
            let tw = interstice_ui::text_width(&s, th::TEXT_BODY, FontStyle::REGULAR);
            let ph = TOPBAR_H - 10.0;
            let pill = rect(((sw - tw - 44.0) / 2.0).round(), (5.0 + (1.0 - a_in) * 4.0).round(), tw + 44.0, ph);
            self.fill(l, cp(&pill), th::alpha(th::RAISED, a), ph / 2.0);
            self.stroke(l, cp(&pill), th::alpha(th::LINE, a), ph / 2.0);
            let dc = if t.ok { th::SUCCESS } else { th::DANGER };
            self.dot(l, pill.x + 17.0, pill.y + ph / 2.0, 3.0, th::alpha(dc, a));
            self.text(l, s, pill.x + 29.0, pill.y + 5.0, th::TEXT_BODY, th::alpha(th::IVORY, a), FontStyle::REGULAR);
        }
    }

    fn launcher(&self, sw: f32, sh: f32, apps: &[AppInfo], since: u64) {
        let l = LAUNCHER_LAYER;
        let a = ease(self.now, since, OPEN_MS);
        self.fill(l, rect(0.0, TOPBAR_H, sw, sh - TOPBAR_H), th::alpha(th::INK, 0.88 * a), 0.0);
        let lay = launcher_layout(sw);
        let rise = ((1.0 - a) * 6.0).round();
        let x_end = lay.x0 + lay.width;

        let hy = TOPBAR_H + 48.0 + rise;
        self.text(l, "Apps", lay.x0, hy, th::TEXT_HEADING, th::alpha(th::IVORY, a), FontStyle::REGULAR);
        let sub = "Open an app in its own window, or start a service in the background.";
        self.text(l, sub, lay.x0, hy + 36.0, th::TEXT_BODY, th::alpha(th::MUTED, a), FontStyle::REGULAR);
        for (label, y) in &lay.sections {
            self.section(l, label, lay.x0, y + rise, x_end, a);
        }

        for (app, mut r) in lay.tiles {
            r.y += rise;
            let running = apps.iter().any(|x| x.name == app.name && x.loaded);
            let hover = self.hover(&r);
            let bg = if hover { th::mix(th::RAISED, th::IVORY, 0.03) } else { th::RAISED };
            self.fill(l, cp(&r), th::alpha(bg, a), th::RADIUS_TILE);
            let border = if hover { th::FOCUS_BORDER } else { th::LINE };
            self.stroke(l, cp(&r), th::alpha(border, border.3 * a), th::RADIUS_TILE);

            // The glyph, on a charcoal well; lit while the app runs.
            let well = rect(r.x + 16.0, r.y + 16.0, 40.0, 40.0);
            self.fill(l, cp(&well), th::alpha(th::CHARCOAL, a), 10.0);
            self.stroke(l, cp(&well), th::alpha(th::LINE, a), 10.0);
            let gc = if running { th::AMBER } else { th::IVORY };
            let glyph = rect(well.x + 8.0, well.y + 8.0, 24.0, 24.0);
            self.image(l, &icon_texture_id(app.name), glyph, th::alpha(gc, a));

            // Status, top right.
            let status = match (app.kind, running) {
                (_, true) => Some(("RUNNING", th::AMBER, th::AMBER)),
                (Kind::Service, false) => Some(("STOPPED", th::GRAY, th::DIM)),
                (Kind::Window, false) => None,
            };
            if let Some((s, text_c, dot_c)) = status {
                let sx = (r.x + r.w - 16.0 - label_width(s)).round();
                self.label(l, s, sx, r.y + 18.0, th::alpha(text_c, a));
                self.dot(l, sx - 9.0, r.y + 25.0, 3.0, th::alpha(dot_c, a));
            }

            let name_c = th::alpha(th::IVORY, a);
            self.text(l, app.title, r.x + 16.0, r.y + 70.0, th::TEXT_READING, name_c, FontStyle::MEDIUM);
            let blurb = fit(app.blurb, th::TEXT_SMALL, FontStyle::REGULAR, r.w - 32.0);
            self.text(l, blurb, r.x + 16.0, r.y + 94.0, th::TEXT_SMALL, th::alpha(th::GRAY, a), FontStyle::REGULAR);
        }
    }
}

// ── Brand textures ────────────────────────────────────────────────────────────

fn label_id(s: &str) -> String {
    format!("label_{s}")
}

fn icon_texture_id(app_name: &str) -> String {
    format!("icon_{app_name}")
}

fn upload<Caps>(ctx: &ReducerContext<Caps>, id: &str, img: &Image) {
    let _ = ctx.graphics().reducers.create_texture(
        id.to_string(),
        TextureDescriptorInput {
            width: img.width,
            height: img.height,
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
        Blob::new(&img.rgba),
    );
}

/// The pointer: an ivory arrow with an ink outline, drawn at 24 px.
fn cursor_image() -> Image {
    let n = 96;
    let arrow = [(0.10, 0.06), (0.10, 0.76), (0.27, 0.61), (0.39, 0.86), (0.50, 0.81), (0.38, 0.57), (0.60, 0.57)];
    let mut outline = Canvas::new(n);
    outline.polygon(&arrow);
    let mut closed = arrow.to_vec();
    closed.push(arrow[0]);
    outline.path(&closed, 0.08);
    let mut fill = Canvas::new(n);
    fill.polygon(&arrow);
    Image { width: n, height: n, rgba: th::compose(&[(&outline, th::INK), (&fill, th::IVORY)]) }
}

// App glyphs: thin monochrome line drawings on a 0..1 square (BRAND.md §7),
// rasterized white and tinted when drawn.
const STROKE: f32 = 0.07;

fn glyph_ui(c: &mut Canvas) {
    c.rrect((0.14, 0.14), (0.32, 0.32), 0.06, STROKE);
    c.rrect((0.54, 0.14), (0.32, 0.32), 0.06, STROKE);
    c.rrect((0.14, 0.54), (0.72, 0.32), 0.06, STROKE);
}

/// A history: commits on a line, one branching off.
fn glyph_notes(c: &mut Canvas) {
    c.line((0.34, 0.20), (0.34, 0.80), STROKE);
    c.path(&[(0.34, 0.50), (0.66, 0.36), (0.66, 0.22)], STROKE);
    c.disc((0.34, 0.20), 0.08);
    c.disc((0.34, 0.50), 0.08);
    c.disc((0.34, 0.80), 0.08);
    c.disc((0.66, 0.22), 0.08);
}

fn glyph_browser(c: &mut Canvas) {
    c.ring((0.5, 0.5), 0.34, STROKE);
    c.line((0.16, 0.5), (0.84, 0.5), STROKE);
    // The meridians: two half-ellipses.
    let half: Vec<(f32, f32)> = (0..=16)
        .map(|i| {
            let t = i as f32 / 16.0 * std::f32::consts::PI;
            (0.5 + 0.15 * t.sin(), 0.5 - 0.34 * t.cos())
        })
        .collect();
    c.path(&half, STROKE);
    let mirror: Vec<(f32, f32)> = half.iter().map(|(x, y)| (1.0 - x, *y)).collect();
    c.path(&mirror, STROKE);
}

fn glyph_agar(c: &mut Canvas) {
    c.ring((0.42, 0.56), 0.26, STROKE);
    c.disc((0.76, 0.26), 0.07);
    c.disc((0.80, 0.70), 0.05);
    c.disc((0.22, 0.20), 0.05);
}

/// A table: a header row and lines.
fn glyph_table(c: &mut Canvas) {
    c.rrect((0.14, 0.18), (0.72, 0.64), 0.08, STROKE);
    c.line((0.14, 0.40), (0.86, 0.40), STROKE);
    c.line((0.14, 0.61), (0.86, 0.61), STROKE);
    c.line((0.42, 0.40), (0.42, 0.82), STROKE);
}

fn glyph_http(c: &mut Canvas) {
    c.line((0.36, 0.18), (0.36, 0.82), STROKE);
    c.path(&[(0.22, 0.68), (0.36, 0.82), (0.50, 0.68)], STROKE);
    c.line((0.64, 0.82), (0.64, 0.18), STROKE);
    c.path(&[(0.50, 0.32), (0.64, 0.18), (0.78, 0.32)], STROKE);
}

fn glyph_audio(c: &mut Canvas) {
    let pts: Vec<(f32, f32)> = (0..=40)
        .map(|i| {
            let t = i as f32 / 40.0;
            let env = (t * std::f32::consts::PI).sin();
            (0.12 + 0.76 * t, 0.5 - 0.24 * (t * std::f32::consts::TAU * 1.5).sin() * env)
        })
        .collect();
    c.path(&pts, STROKE);
}

// ── Apps ──────────────────────────────────────────────────────────────────────

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
