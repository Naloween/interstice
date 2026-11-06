//! A Firefox-style web browser, the flagship Interstice example.
//!
//! The web engine is [Blitz](https://github.com/DioxusLabs/blitz): html5ever
//! parsing, Servo's Stylo CSS cascade, Taffy layout and Parley text, all pure
//! Rust so they build for `wasm32-unknown-unknown`. Pages are rasterised on the
//! CPU (vello_cpu) and uploaded as textures to the `graphics` module. The browser
//! chrome (tab strip, toolbar, URL bar) is itself an HTML document rendered by
//! the same engine (see [`chrome`]).
//!
//! The module never touches the NIC: every request goes through the shared
//! `network` broker and comes back as an `HttpResponse` row.
//!
//! All browser state is in tables ([`model`]): tabs and history (stateful,
//! restored on restart), the pointer/focus/URL-field state, documents, their
//! input logs, requests and the sub-resource cache (ephemeral). Parsed Blitz
//! documents are instance caches derived from those rows ([`cache`]), so any
//! pool instance can serve any reducer.
//!
//! - [`model`] — the tables.
//! - [`engine`] — tabs, history, network plumbing, input routing, painting.
//! - [`cache`] — parsed documents (instance caches).
//! - [`chrome`] — the Proton-style toolbar document + its event handler.
//! - [`providers`] — Blitz net/navigation/shell providers (queue-based).
//! - [`pages`] — built-in `about:` and error pages.
//! - [`keys`] — Interstice key codes → DOM keyboard events.
//! - [`fonts`] — embedded fonts (no system font discovery on wasm).

use crate::bindings::{graphics::*, input::*, network::*};
use crate::cache::{Caches, CACHES};
use crate::model::*;
use interstice_sdk::*;

mod cache;
mod chrome;
mod cursor;
mod encoding;
mod engine;
mod fonts;
mod js;
mod keys;
mod model;
mod pages;
mod providers;

use engine::{Browser, Store};

interstice_module!(visibility: Public, authorities: [File]);

const LAYER: &str = "browser";
const OVERLAY_LAYER: &str = "browser_overlay";
const CURSOR_LAYER: &str = "browser_cursor";

// Blitz/Stylo pull `getrandom` 0.3, which needs a custom backend on
// wasm32-unknown-unknown (the workspace sets `getrandom_backend="custom"`).
// Nothing here needs cryptographic randomness: a splitmix64 stream will do.
thread_local! {
    static RNG_STATE: std::cell::Cell<u64> = const { std::cell::Cell::new(0x9E3779B97F4A7C15) };
}

#[unsafe(no_mangle)]
unsafe extern "Rust" fn __getrandom_v03_custom(
    dest: *mut u8,
    len: usize,
) -> Result<(), getrandom::Error> {
    RNG_STATE.with(|s| {
        let mut x = s.get();
        let mut i = 0;
        while i < len {
            x = x.wrapping_add(0x9E3779B97F4A7C15);
            let mut z = x;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
            z ^= z >> 31;
            for b in z.to_le_bytes() {
                if i < len {
                    unsafe { *dest.add(i) = b };
                    i += 1;
                }
            }
        }
        s.set(x);
    });
    Ok(())
}

fn log(msg: &str) {
    interstice_sdk::host_calls::log(msg);
}

/// The transaction's timestamp in ms (constant within one reducer run).
pub fn now_ms() -> f64 {
    interstice_sdk::host_calls::time_now_ms().unwrap_or(0) as f64
}

fn set_clock() -> f64 {
    let ms = interstice_sdk::host_calls::time_now_ms().unwrap_or(0);
    web_time::set_now(std::time::Duration::from_millis(ms));
    ms as f64 / 1000.0
}

// ── Table access ─────────────────────────────────────────────────────────────

caps! {
    /// Full access to every browser table, for the
    /// reducers that drive the engine. The small input reducers declare just
    /// what they touch, so the scheduler can run them alongside a frame.
    pub struct BrowserCaps {
        all: [Session, Tab, Ui, Document, DocEvent, PendingInput, Request, Resource, ResourceMeta, ScriptedInput, WebStorage],
    }
}

type Ctx = ReducerContext<BrowserCaps>;

struct Db<'a>(&'a Ctx);

/// `Store` methods can't fail (the engine calls them from Blitz/Boa callbacks),
/// so a failed table read is logged and treated as empty.
fn read_or_default<T: Default>(r: Result<T, String>) -> T {
    r.unwrap_or_else(|err| {
        log(&format!("browser: table read failed: {err}"));
        T::default()
    })
}

impl Store for Db<'_> {
    fn document(&self, id: u64) -> Option<Document> {
        read_or_default(self.0.current.tables.document().get(id))
    }
    fn insert_document(&self, doc: Document) {
        let _ = self.0.current.tables.document().insert(doc);
    }
    fn drop_document(&self, id: u64) {
        let t = &self.0.current.tables;
        let _ = t.document().delete(id);
        for e in read_or_default(t.docevent().scan_by_doc_eq(id)) {
            let _ = t.docevent().delete(e.seq);
        }
    }
    fn doc_events(&self, doc: u64) -> Vec<DocEvent> {
        let mut v = read_or_default(self.0.current.tables.docevent().scan_by_doc_eq(doc));
        v.sort_by_key(|e| e.seq);
        v
    }
    fn push_doc_event(&self, ev: DocEvent) -> u64 {
        self.0.current.tables.docevent().insert(ev).map(|r| r.seq).unwrap_or(0)
    }
    fn clear_doc_events(&self, doc: u64) {
        let t = &self.0.current.tables;
        for e in read_or_default(t.docevent().scan_by_doc_eq(doc)) {
            let _ = t.docevent().delete(e.seq);
        }
    }
    fn resource(&self, url: &str) -> Option<Vec<u8>> {
        read_or_default(self.0.current.tables.resource().get(url.to_string())).map(|r| r.body)
    }
    fn has_resource(&self, url: &str) -> bool {
        read_or_default(self.0.current.tables.resourcemeta().get(url.to_string())).is_some()
    }
    fn insert_resource(&self, url: &str, body: Vec<u8>, stamp: u64) {
        let t = &self.0.current.tables;
        let size = body.len() as u64;
        let _ = t.resource().insert(Resource { url: url.to_string(), body });
        let _ = t.resourcemeta().insert(ResourceMeta { url: url.to_string(), size, stamp });
    }
    fn evict_oldest_resource(&self) -> Option<u64> {
        let t = &self.0.current.tables;
        let oldest = read_or_default(t.resourcemeta().scan()).into_iter().min_by_key(|m| m.stamp)?;
        let _ = t.resource().delete(oldest.url.clone());
        let _ = t.resourcemeta().delete(oldest.url);
        Some(oldest.size)
    }
    fn request_pending_for(&self, url: &str) -> bool {
        !read_or_default(self.0.current.tables.request().scan_by_url_eq(url.to_string())).is_empty()
    }
    fn insert_request(&self, req: Request) {
        let _ = self.0.current.tables.request().insert(req);
    }
    fn delete_request(&self, req_id: u64) {
        let _ = self.0.current.tables.request().delete(req_id);
    }
    fn web_storage(&self, origin: &str) -> Vec<(String, String)> {
        let rows = read_or_default(self.0.current.tables.webstorage().scan_by_origin_eq(origin.to_string()));
        rows.into_iter().map(|r| (r.name, r.value)).collect()
    }
    fn set_web_storage(&self, origin: &str, name: &str, value: Option<String>) {
        let t = &self.0.current.tables;
        let key = format!("{origin}\0{name}");
        match value {
            None => {
                let _ = t.webstorage().delete(key);
            }
            Some(value) => {
                let row = WebStorage { key, origin: origin.to_string(), name: name.to_string(), value };
                if t.webstorage().update(row.clone()).is_err() {
                    let _ = t.webstorage().insert(row);
                }
            }
        }
    }
}

type Rows = (Session, Ui, Vec<Tab>);

fn load_rows(ctx: &Ctx) -> Result<Rows, String> {
    let t = &ctx.current.tables;
    let session = t.session().get(SESSION)?.unwrap_or_else(|| Session { id: SESSION, ..Default::default() });
    let ui = t.ui().get(UI)?.unwrap_or_else(|| Ui { id: UI, ..Default::default() });
    Ok((session, ui, t.tab().scan()?))
}

/// Write back what a [`Browser`] changed.
fn save_rows(ctx: &Ctx, before: &Rows, after: Rows) {
    let t = &ctx.current.tables;
    let (session, ui, tabs) = after;
    if session != before.0 && t.session().update(session.clone()).is_err() {
        let _ = t.session().insert(session);
    }
    if ui != before.1 && t.ui().update(ui.clone()).is_err() {
        let _ = t.ui().insert(ui);
    }
    for tab in &before.2 {
        if !tabs.iter().any(|n| n.id == tab.id) {
            let _ = t.tab().delete(tab.id);
        }
    }
    for tab in tabs {
        match before.2.iter().find(|o| o.id == tab.id) {
            Some(old) if *old == tab => {}
            Some(_) => {
                let _ = t.tab().update(tab);
            }
            None => {
                let _ = t.tab().insert(tab);
            }
        }
    }
}

/// Run browser logic over the tables: load rows, run `f`, save changes.
fn with_browser<R>(ctx: &Ctx, f: impl FnOnce(&mut Browser) -> R) -> Result<R, String> {
    let before = load_rows(ctx)?;
    let db = Db(ctx);
    let r = CACHES.with(|c| {
        c.with(Caches::default, |caches| {
            let (s, u, t) = before.clone();
            let mut b = Browser::new(&db, caches, s, u, t);
            let r = f(&mut b);
            save_rows(ctx, &before, b.into_rows());
            r
        })
    });
    Ok(r)
}

// ── Lifecycle ────────────────────────────────────────────────────────────────

#[reducer(on = "load")]
pub fn on_load(ctx: ReducerContext<BrowserCaps>) -> Result<(), String>
{
    let g = ctx.graphics();
    let _ = g.reducers.create_layer(LAYER.to_string(), 100, false);
    let _ = g.reducers.create_layer(OVERLAY_LAYER.to_string(), 500, false);
    let _ = g.reducers.create_layer(CURSOR_LAYER.to_string(), 1000, false);

    let home = interstice_sdk::host_calls::read_file("browser_home".into())
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "about:newtab".to_string());
    log(&format!("browser: homepage = {home:?}"));

    let t = &ctx.current.tables;
    let _ = t.scriptedinput().clear();
    // Headless test script: `<frame> <command…>` per line, `#` comments.
    if let Some(text) = interstice_sdk::host_calls::read_file("browser_script".into())
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
    {
        for l in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let (frame, command) = l.split_once(char::is_whitespace).unwrap_or((l, ""));
            if let Ok(frame) = frame.parse() {
                let _ = t.scriptedinput().insert(ScriptedInput { id: 0, frame, command: command.trim().to_string() });
            }
        }
    }

    let session = match t.session().get(SESSION)? {
        Some(mut s) => {
            s.home = home;
            s
        }
        None => Session { id: SESSION, next_tab: 1, next_doc: 1, next_req: 1, home, ..Default::default() },
    };
    if t.session().update(session.clone()).is_err() {
        let _ = t.session().insert(session);
    }
    if t.ui().get(UI)?.is_none() {
        let _ = t.ui().insert(Ui { id: UI, next_stamp: 2, ..Default::default() });
    }

    with_browser(&ctx, |b| {
        if b.tabs.is_empty() {
            let home = b.session.home.clone();
            b.new_tab(&home);
        } else {
            // Tabs survived a restart (stateful); their documents did not.
            b.restore();
        }
    })
}

// ── Network ──────────────────────────────────────────────────────────────────

/// `my_responses` shows the browser its own responses only (DESIGN §7.5).
#[reducer(on = "network.my_responses.insert")]
pub fn on_http(ctx: ReducerContext<BrowserCaps>, row: MyResponses) -> Result<(), String>
{
    let requests = ctx.current.tables.request();
    let Some(req) = requests.get(row.req_id)? else { return Ok(()) };
    let _ = requests.delete(row.req_id);
    set_clock();
    let final_url = if row.final_host.is_empty() {
        None
    } else {
        let scheme = if row.final_tls { "https" } else { "http" };
        Some(format!("{scheme}://{}{}", row.final_host, row.final_path))
    };
    with_browser(&ctx, |b| b.on_response(req, row.body, &row.error, final_url))
}

// ── Input ────────────────────────────────────────────────────────────────────
// Input reducers only queue events; `on_frame` applies them in order.

fn queue<Caps: CanInsert<PendingInput>>(ctx: &ReducerContext<Caps>, kind: u8, code: u32, pressed: bool, repeat: bool) {
    queue_key(ctx, kind, code, pressed, repeat, String::new(), String::new());
}

fn queue_key<Caps: CanInsert<PendingInput>>(
    ctx: &ReducerContext<Caps>, kind: u8, code: u32, pressed: bool, repeat: bool, key: String, text: String,
) {
    let _ = ctx.current.tables.pendinginput().insert(PendingInput { seq: 0, kind, code, pressed, repeat, key, text });
}

// Input events reach every subscriber; with a desktop routing input
// (DESIGN §12.2), presses are ours only while we have the keyboard / the
// pointer. Releases always pass, so focus moving mid-press leaves no key stuck.

fn has_keyboard<Caps>(ctx: &ReducerContext<Caps>) -> bool {
    ctx.input().queries.input_view().map(|v| v.keyboard_focus).unwrap_or(true)
}

fn has_pointer<Caps>(ctx: &ReducerContext<Caps>) -> bool {
    ctx.input().queries.input_view().map(|v| v.pointer_inside).unwrap_or(true)
}

#[reducer(on = "input.keystate.insert")]
pub fn on_key_insert<Caps>(ctx: ReducerContext<Caps>, row: KeyState)
where
    Caps: CanInsert<PendingInput>,
{
    if row.pressed && !has_keyboard(&ctx) {
        return;
    }
    queue_key(&ctx, IN_KEY, row.code, row.pressed, false, row.key, row.text);
}

#[reducer(on = "input.keystate.update")]
pub fn on_key_update<Caps>(ctx: ReducerContext<Caps>, prev: KeyState, new: KeyState)
where
    Caps: CanInsert<PendingInput>,
{
    if new.pressed && !has_keyboard(&ctx) {
        return;
    }
    // winit key repeat arrives as pressed → pressed.
    let repeat = prev.pressed && new.pressed;
    queue_key(&ctx, IN_KEY, new.code, new.pressed, repeat, new.key, new.text);
}

#[reducer(on = "input.mousebutton.insert")]
pub fn on_button_insert<Caps>(ctx: ReducerContext<Caps>, row: MouseButton)
where
    Caps: CanInsert<PendingInput>,
{
    if row.pressed && !has_pointer(&ctx) {
        return;
    }
    queue(&ctx, IN_BUTTON, row.button_id as u32, row.pressed, false);
}

#[reducer(on = "input.mousebutton.update")]
pub fn on_button_update<Caps>(ctx: ReducerContext<Caps>, prev: MouseButton, new: MouseButton)
where
    Caps: CanInsert<PendingInput>,
{
    if prev.pressed == new.pressed || (new.pressed && !has_pointer(&ctx)) {
        return;
    }
    queue(&ctx, IN_BUTTON, new.button_id as u32, new.pressed, false);
}

// ── Frame ────────────────────────────────────────────────────────────────────

#[reducer(on = "graphics.frametick.update")]
pub fn on_frame(ctx: ReducerContext<BrowserCaps>, _prev: FrameTick, tick: FrameTick) -> Result<(), String>
{
    let now = set_clock();
    let Ok(info) = ctx.graphics().queries.surface_info() else { return Ok(()) };
    let (w, h) = (info.width, info.height);
    if w < 64 || h < engine::content_height(0) + 64 {
        return Ok(());
    }
    // The pointer as routed to us (surface-local inside a desktop window).
    let view = ctx.input().queries.input_view()?;
    let t = &ctx.current.tables;
    let mut inputs = t.pendinginput().scan()?;
    inputs.sort_by_key(|e| e.seq);
    for e in &inputs {
        let _ = t.pendinginput().delete(e.seq);
    }
    let mut script: Vec<ScriptedInput> = t.scriptedinput().scan()?.into_iter().filter(|c| c.frame <= tick.frame).collect();
    script.sort_by_key(|c| (c.frame, c.id));
    for c in &script {
        let _ = t.scriptedinput().delete(c.id);
    }

    let mut uploads: Vec<Upload> = Vec::new();
    let mut overlay: Option<String> = None;
    let mut cursor = (0.0, 0.0, cursor_icon::CursorIcon::Default);

    with_browser(&ctx, |b| {
        b.ui.viewport = (w, h);
        // The active document and the chrome must reflect the tables before
        // input is applied to them.
        b.ensure_active_doc(now);
        b.sync_chrome();

        if info.id == 0 {
            // Standalone: integrate deltas of the raw position into our own
            // clamped cursor.
            let raw = view.position;
            let (mut x, mut y) = b.ui.mouse;
            match b.ui.last_raw {
                Some(prev) => {
                    x += raw.0 - prev.0;
                    y += raw.1 - prev.1;
                }
                None => (x, y) = raw,
            }
            b.ui.last_raw = Some(raw);
            b.mouse_move(x.clamp(0.0, w as f32 - 1.0), y.clamp(0.0, h as f32 - 1.0));
        } else if view.pointer_inside {
            // In a window: the desktop's cursor is the cursor, already local.
            let (x, y) = view.position;
            b.mouse_move(x.clamp(0.0, w as f32 - 1.0), y.clamp(0.0, h as f32 - 1.0));
        }

        // The wheel total is cumulative: act on the increment, only while
        // the pointer is over us.
        let wheel = view.wheel_delta;
        if let Some(prev) = b.ui.last_wheel {
            let (dx, dy) = (wheel.0 - prev.0, wheel.1 - prev.1);
            if (dx != 0.0 || dy != 0.0) && view.pointer_inside {
                // Positive wheel delta = scroll up (content moves down).
                b.wheel(-dx, -dy);
            }
        }
        b.ui.last_wheel = Some(wheel);

        // Scripted input for headless testing.
        for c in &script {
            b.script(&c.command, tick.frame, now);
        }
        for e in &inputs {
            b.input(e);
        }

        b.update(now);

        if let Some(px) = b.paint_chrome() {
            uploads.push(Upload::new(&mut b.ui.chrome_tex, &mut b.ui.chrome_tex_size, &mut b.ui.tex_gen, "chrome", (w, chrome::CHROME_H), px));
        }
        if let Some(p) = b.paint_content() {
            let size = (w, engine::content_height(h));
            let mut u = Upload::new(&mut b.ui.content_tex, &mut b.ui.content_tex_size, &mut b.ui.tex_gen, "content", size, p.pixels);
            // `paint_content` only returns a partial rect while the texture
            // is current, so this never needs to create one.
            u.rect = (p.x, p.y, p.w, p.h);
            debug_assert!(!u.create || u.rect == (0, 0, size.0, size.1));
            uploads.push(u);
        }
        overlay = b.hovered_link();
        cursor = (b.ui.mouse.0, b.ui.mouse.1, b.cursor());
    })?;

    // Hand new requests to the network broker. Done here rather than where
    // they are created so a reducer triggered by the broker (`on_http`) never
    // calls back into it.
    for mut r in t.request().scan()?.into_iter().filter(|r| !r.sent) {
        match engine::parse_url(&r.url) {
            Some((host, path, tls)) => {
                let _ = ctx.network().reducers.http_get(r.req_id, host, path, tls);
                r.sent = true;
                let _ = t.request().update(r);
            }
            None => {
                let _ = t.request().delete(r.req_id);
            }
        }
    }

    let g = ctx.graphics();
    for u in uploads {
        if let Some(old) = u.destroy {
            let _ = g.reducers.destroy_resource(old);
        }
        if u.create {
            let _ = g.reducers.create_texture(u.id, texture_desc(u.size.0, u.size.1), Blob::new(&u.pixels));
        } else {
            let (x, y, w, h) = u.rect;
            let _ = g.reducers.update_texture(u.id, x, y, w, h, Blob::new(&u.pixels));
        }
    }

    // Composite: chrome on top, content below it.
    let full = || Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 };
    let white = || Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
    if let Some(ui) = t.ui().get(UI)? {
        if !ui.content_tex.is_empty() {
            let (tw, th) = ui.content_tex_size;
            let rect = Rect { x: 0.0, y: chrome::CHROME_H as f32, w: tw as f32, h: th as f32 };
            let _ = g.reducers.draw_image(LAYER.into(), ui.content_tex.clone(), rect, white(), full());
        }
        if !ui.chrome_tex.is_empty() {
            let (tw, th) = ui.chrome_tex_size;
            let rect = Rect { x: 0.0, y: 0.0, w: tw as f32, h: th as f32 };
            let _ = g.reducers.draw_image(LAYER.into(), ui.chrome_tex.clone(), rect, white(), full());
        }
    }

    // Link status panel (bottom-left), like Firefox.
    if let Some(link) = overlay {
        let text: String = if link.chars().count() > 120 {
            link.chars().take(117).collect::<String>() + "..."
        } else {
            link
        };
        let size = 12.0;
        let tw = text.chars().count() as f32 * 6.6 + 14.0;
        let (x, y) = (0.0, h as f32 - 22.0);
        let _ = g.reducers.draw_rect(
            OVERLAY_LAYER.into(),
            Rect { x, y, w: tw.min(w as f32), h: 22.0 },
            Color { r: 0.94, g: 0.94, b: 0.957, a: 1.0 },
            true,
            0.0,
            Some(3.0),
        );
        let _ = g.reducers.draw_text(
            OVERLAY_LAYER.into(),
            text,
            Vec2 { x: x + 7.0, y: y + 5.0 },
            size,
            Color { r: 0.08, g: 0.08, b: 0.1, a: 1.0 },
            None,
        );
    }

    if info.id == 0 {
        draw_cursor(&ctx, cursor.0, cursor.1, cursor.2)?;
    }
    Ok(())
}

/// A texture write for the graphics module. Texture ids/sizes live in `Ui`.
struct Upload {
    id: String,
    size: (u32, u32),
    create: bool,
    destroy: Option<String>,
    /// Region of the texture `pixels` cover (the whole texture on create).
    rect: (u32, u32, u32, u32),
    pixels: Vec<u8>,
}

impl Upload {
    fn new(slot: &mut String, slot_size: &mut (u32, u32), gen_: &mut u64, name: &str, size: (u32, u32), pixels: Vec<u8>) -> Self {
        if !slot.is_empty() && *slot_size == size {
            return Upload { id: slot.clone(), size, create: false, destroy: None, rect: (0, 0, size.0, size.1), pixels };
        }
        *gen_ += 1;
        let id = format!("browser_{name}_{gen_}");
        let destroy = (!slot.is_empty()).then(|| std::mem::replace(slot, id.clone()));
        *slot = id.clone();
        *slot_size = size;
        Upload { id, size, create: true, destroy, rect: (0, 0, size.0, size.1), pixels }
    }
}

// ── Cursor ───────────────────────────────────────────────────────────────────

fn draw_cursor<Caps>(ctx: &ReducerContext<Caps>, x: f32, y: f32, icon: cursor_icon::CursorIcon) -> Result<(), String>
where
    Caps: CanRead<Ui> + CanUpdate<Ui>,
{
    let g = ctx.graphics();
    let t = &ctx.current.tables;
    let Some(mut ui) = t.ui().get(UI)? else { return Ok(()) };
    if ui.cursor_tex.is_empty() {
        let (w, h) = cursor::atlas_size();
        ui.tex_gen += 1;
        ui.cursor_tex = format!("browser_cursor_{}", ui.tex_gen);
        let _ = g.reducers.create_texture(ui.cursor_tex.clone(), texture_desc(w, h), Blob::new(&cursor::atlas()));
        let _ = t.ui().update(ui.clone());
    }
    let (w, _) = cursor::atlas_size();
    let (cell, (hx, hy)) = cursor::cell(icon);
    let size = cursor::CELL as f32;
    let _ = g.reducers.draw_image(
        CURSOR_LAYER.into(),
        ui.cursor_tex,
        // Whole pixels: a fractional origin would blur the atlas (bilinear).
        Rect { x: (x - hx).round(), y: (y - hy).round(), w: size, h: size },
        Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
        Rect { x: (cell * cursor::CELL) as f32 / w as f32, y: 0.0, w: size / w as f32, h: 1.0 },
    );
    Ok(())
}

fn texture_desc(width: u32, height: u32) -> TextureDescriptorInput {
    TextureDescriptorInput {
        width,
        height,
        format: "rgba8unorm_srgb".into(),
        mip_levels: 1,
        sample_count: 1,
        usage: TextureUsageFlags {
            copy_src: false,
            copy_dst: true,
            texture_binding: true,
            storage_binding: false,
            render_attachment: false,
        },
    }
}
