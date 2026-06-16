//! A minimal web browser, the flagship Interstice example. It proves the OS
//! model end-to-end: it fetches pages over the shared `network` broker (no NIC
//! authority of its own), parses static HTML with `tl`, translates the DOM into a
//! `UiElement` tree, and renders through the graphics ABI like any other app —
//! routed to its own surface by the desktop compositor.
//!
//! Scope (v1): static HTML only (no JavaScript / CSS engine). Styling is a tiny
//! built-in user-agent stylesheet (see [`style`]). Images are really fetched and
//! decoded (PNG/JPEG) and uploaded as textures. See the plan's deferred list for
//! what's intentionally out of scope.

use crate::bindings::{graphics::*, input::*, network::*};
use interstice_sdk::key_code::KeyCode;
use interstice_sdk::*;

mod html;
mod style;
mod url;

use html::Block;

interstice_module!(visibility: Public);

// Module-local UI subsystem (own tables, helpers, render + key reducer), wired to
// this module's own graphics/input bindings so the compositor can route us to our
// own surface.
interstice_ui::ui_subsystem!();

use crate::ui::*;

/// Our schema name — the broker stamps `HttpResponse.owner` with the caller, so we
/// ignore responses belonging to other apps.
const ME: &str = "browser-example";

const ROOT_BG: (f32, f32, f32, f32) = (0.08, 0.08, 0.10, 1.0);
const BAR_BG: (f32, f32, f32, f32) = (0.16, 0.16, 0.20, 1.0);
const BAR_TEXT: (f32, f32, f32, f32) = (0.92, 0.92, 0.95, 1.0);
const VIEWPORT_BG: (f32, f32, f32, f32) = (0.11, 0.11, 0.14, 1.0);
const TRANSPARENT: (f32, f32, f32, f32) = (0.0, 0.0, 0.0, 0.0);
const IMG_PLACEHOLDER_BG: (f32, f32, f32, f32) = (0.16, 0.16, 0.20, 1.0);

const URLBAR_ID: &str = "urlbar";
const VIEWPORT_ID: &str = "viewport";

/// Cap on distinct image fetches per page. Keeps a pathological page (hundreds of
/// unique-URL images) from exhausting GPU textures or flooding the broker; v1 just
/// drops the surplus. Repeated images share a fetch and don't count against this.
const MAX_PAGE_IMAGES: usize = 40;

// ── Module state ─────────────────────────────────────────────────────────────

/// Navigation singleton (`id` always 0). `nav_gen` is bumped on every navigation
/// so freshly created content elements get collision-free ids (`c{gen}_{i}`),
/// letting `on_http` delete the previous page and insert the next in one run
/// without primary-key clashes. `prev_left` / `prev_enter` hold last frame's
/// input level so we can derive press edges.
#[table]
pub struct NavState {
    #[primary_key]
    pub id: u32,
    pub url: String,
    pub host: String,
    pub path: String,
    pub main_req_id: u64,
    pub next_req_id: u64,
    pub nav_gen: u32,
    pub prev_left: bool,
    pub prev_enter: bool,
}

/// Maps a clickable content element id to the link's (unresolved) href.
#[table]
pub struct LinkMap {
    #[primary_key]
    pub element_id: String,
    pub href: String,
}

/// An in-flight image fetch, one per *distinct* resolved URL on the page. Many
/// `<img>` elements often point at the same URL (icons, repeated graphics), so we
/// fetch and decode each URL just once and share the resulting texture.
#[table]
pub struct ImageReq {
    #[primary_key]
    pub req_id: u64,
    pub url: String,
}

/// An element waiting on an `ImageReq`: when the fetch decodes, every waiter for
/// that `req_id` gets the shared texture.
#[table]
pub struct ImageWaiter {
    #[primary_key]
    pub element_id: String,
    pub req_id: u64,
}

// ── Lifecycle ────────────────────────────────────────────────────────────────

#[reducer(on = "load")]
pub fn on_load<Caps>(ctx: ReducerContext<Caps>)
where
    Caps: CanInsert<NavState>
        + CanInsert<ui::UiElement>
        + CanUpdate<ui::UiElement>
        + CanInsert<ui::InputFocus>
        + CanUpdate<ui::InputFocus>,
{
    ui::install(&ctx);

    // Root column: URL bar on top, scrollable page viewport below.
    ui::create_element(
        &ctx,
        UiElement {
            id: "root".into(),
            parent: None,
            order: 0,
            width: Size::Grow,
            height: Size::Grow,
            layout_direction: LayoutDirection::Column,
            gap: 0.0,
            padding: 0.0,
            margin: 0.0,
            background_color: ROOT_BG,
            corner_radius: 0.0,
            border_width: 0.0,
            border_color: TRANSPARENT,
            text: None,
            text_size: 0.0,
            text_color: TRANSPARENT,
            text_wrap: TextWrap::Words,
            image: None,
            is_input: false,
            cursor_pos: 0,
            scrollable_x: false,
            scrollable_y: false,
            scroll_x: 0.0,
            scroll_y: 0.0,
            visible: true,
        },
    );

    // URL bar — an editable text input. Focused below so typing edits it and
    // Enter navigates. Starts empty; the user types a URL to load a page.
    ui::create_element(
        &ctx,
        UiElement {
            id: URLBAR_ID.into(),
            parent: Some("root".into()),
            order: 0,
            width: Size::Grow,
            height: Size::Fixed(30.0),
            layout_direction: LayoutDirection::Row,
            gap: 0.0,
            padding: 7.0,
            margin: 6.0,
            background_color: BAR_BG,
            corner_radius: 6.0,
            border_width: 1.0,
            border_color: (0.30, 0.30, 0.36, 1.0),
            text: Some(String::new()),
            text_size: 14.0,
            text_color: BAR_TEXT,
            text_wrap: TextWrap::None,
            image: None,
            is_input: true,
            cursor_pos: 0,
            scrollable_x: false,
            scrollable_y: false,
            scroll_x: 0.0,
            scroll_y: 0.0,
            visible: true,
        },
    );

    // Viewport — vertically scrollable, holds the rendered page content.
    ui::create_element(
        &ctx,
        UiElement {
            id: VIEWPORT_ID.into(),
            parent: Some("root".into()),
            order: 1,
            width: Size::Grow,
            height: Size::Grow,
            layout_direction: LayoutDirection::Column,
            gap: 0.0,
            padding: 18.0,
            margin: 0.0,
            background_color: VIEWPORT_BG,
            corner_radius: 0.0,
            border_width: 0.0,
            border_color: TRANSPARENT,
            text: None,
            text_size: 0.0,
            text_color: TRANSPARENT,
            text_wrap: TextWrap::Words,
            image: None,
            is_input: false,
            cursor_pos: 0,
            scrollable_x: false,
            scrollable_y: true,
            scroll_x: 0.0,
            scroll_y: 0.0,
            visible: true,
        },
    );

    ui::set_focus(&ctx, URLBAR_ID);

    // Start blank — no page loaded. The user types a URL and presses Enter to
    // navigate (handled in `on_frame`).
    let nav = NavState {
        id: 0,
        url: String::new(),
        host: String::new(),
        path: String::new(),
        main_req_id: 0,
        next_req_id: 1,
        nav_gen: 0,
        prev_left: false,
        prev_enter: false,
    };
    if let Err(err) = ctx.current.tables.navstate().insert(nav) {
        ctx.log(&format!("browser: failed to seed NavState: {err}"));
    }
}

/// Point navigation at `raw_url`: parse it, stamp the new host/path/url, allocate
/// a request id, bump the generation, and ask the broker to fetch it. Mutates the
/// caller's NavState in place (written back once by the caller).
fn start_navigation<Caps>(ctx: &ReducerContext<Caps>, nav: &mut NavState, raw_url: &str) {
    let Some(loc) = url::parse(raw_url) else {
        ctx.log(&format!("browser: cannot parse url `{raw_url}`"));
        return;
    };
    let req_id = nav.next_req_id;
    nav.next_req_id += 1;
    nav.main_req_id = req_id;
    nav.nav_gen += 1;
    nav.host = loc.host.clone();
    nav.path = loc.path.clone();
    nav.url = loc.to_url();
    if let Err(err) = ctx
        .network()
        .reducers
        .http_get(req_id, loc.host, loc.path)
    {
        ctx.log(&format!("browser: http_get failed: {err}"));
    }
}

// ── Network responses ────────────────────────────────────────────────────────

#[reducer(on = "network.httpresponse.insert")]
pub fn on_http<Caps>(ctx: ReducerContext<Caps>, row: HttpResponse)
where
    Caps: CanRead<NavState>
        + CanUpdate<NavState>
        + CanRead<ui::UiElement>
        + CanInsert<ui::UiElement>
        + CanUpdate<ui::UiElement>
        + CanDelete<ui::UiElement>
        + CanRead<LinkMap>
        + CanInsert<LinkMap>
        + CanDelete<LinkMap>
        + CanRead<ImageReq>
        + CanInsert<ImageReq>
        + CanDelete<ImageReq>
        + CanRead<ImageWaiter>
        + CanInsert<ImageWaiter>
        + CanDelete<ImageWaiter>,
{
    if row.owner != ME {
        return; // another app's request
    }
    let Some(mut nav) = ctx.current.tables.navstate().get(0) else {
        return;
    };

    if row.req_id == nav.main_req_id {
        render_page(&ctx, &mut nav, &row);
    } else if ctx.current.tables.imagereq().get(row.req_id).is_some() {
        // An image fetch completed. Drop the request record (so a duplicate
        // delivery is ignored) and hand the decoded texture to every element that
        // was waiting on this URL.
        let _ = ctx.current.tables.imagereq().delete(row.req_id);
        place_image(&ctx, row.req_id, &row.body, row.error.is_empty());
    }
}

/// Replace the viewport contents with the freshly fetched page.
fn render_page<Caps>(ctx: &ReducerContext<Caps>, nav: &mut NavState, row: &HttpResponse)
where
    Caps: CanRead<ui::UiElement>
        + CanInsert<ui::UiElement>
        + CanUpdate<ui::UiElement>
        + CanDelete<ui::UiElement>
        + CanRead<LinkMap>
        + CanInsert<LinkMap>
        + CanDelete<LinkMap>
        + CanRead<ImageReq>
        + CanInsert<ImageReq>
        + CanDelete<ImageReq>
        + CanRead<ImageWaiter>
        + CanInsert<ImageWaiter>
        + CanDelete<ImageWaiter>
        + CanUpdate<NavState>,
{
    // Tear down the previous page: viewport children, link map, image bookkeeping.
    // All are committed (older generation), so these are pure deletes — the new
    // elements we insert below carry the bumped generation in their ids and so
    // never collide with the rows still being deleted this run.
    for el in ctx.current.tables.uielement().scan() {
        if el.parent.as_deref() == Some(VIEWPORT_ID) {
            let _ = ctx.current.tables.uielement().delete(el.id);
        }
    }
    for lm in ctx.current.tables.linkmap().scan() {
        let _ = ctx.current.tables.linkmap().delete(lm.element_id);
    }
    for r in ctx.current.tables.imagereq().scan() {
        let _ = ctx.current.tables.imagereq().delete(r.req_id);
    }
    for w in ctx.current.tables.imagewaiter().scan() {
        let _ = ctx.current.tables.imagewaiter().delete(w.element_id);
    }

    // Reset scroll to the top of the new page.
    if let Some(mut vp) = ctx.current.tables.uielement().get(VIEWPORT_ID.to_string()) {
        vp.scroll_y = 0.0;
        let _ = ctx.current.tables.uielement().update(vp);
    }

    // Bump the generation for every render, not just every navigation. The teardown
    // above deletes the previous page's (committed) elements while we insert the new
    // ones in the same run; giving the new elements a fresh generation guarantees
    // their ids can't collide with the rows still being deleted — even if this same
    // response is delivered (or retried) more than once.
    nav.nav_gen += 1;
    let generation = nav.nav_gen;

    if !row.error.is_empty() {
        ui::create_element(
            ctx,
            text_el(
                format!("c{generation}_0"),
                0,
                format!("Failed to load {}: {}", nav.url, row.error),
                15.0,
                (0.90, 0.45, 0.45, 1.0),
                0.0,
            ),
        );
        ctx.log(&format!("browser: load error for {}: {}", nav.url, row.error));
        return;
    }

    let html = String::from_utf8_lossy(&row.body);
    let blocks = html::parse_html(&html);

    // Maps a resolved image URL to the in-flight request id fetching it, so repeated
    // images on the page share a single fetch + decoded texture.
    let mut url_reqs: Vec<(String, u64)> = Vec::new();
    let mut next_req = nav.next_req_id;
    for (i, block) in blocks.iter().enumerate() {
        let id = format!("c{generation}_{i}");
        match block {
            Block::Text { text, size, color, indent, href } => {
                ui::create_element(
                    ctx,
                    text_el(id.clone(), i as u32, text.clone(), *size, *color, *indent),
                );
                if let Some(href) = href {
                    let _ = ctx.current.tables.linkmap().insert(LinkMap {
                        element_id: id,
                        href: href.clone(),
                    });
                }
            }
            Block::Space { height } => {
                ui::create_element(ctx, space_el(id, i as u32, *height));
            }
            Block::Image { url } => {
                let Some(loc) = url::resolve(&nav.host, &nav.path, url) else {
                    continue;
                };
                let key = loc.to_url();
                // Reuse an existing fetch for the same URL, or start a new one —
                // but cap distinct fetches per page so a pathological page (e.g.
                // hundreds of unique-URL images) can't exhaust the GPU or flood the
                // broker. Repeated images share a fetch and don't count against it.
                let req_id = match url_reqs.iter().find(|(u, _)| *u == key) {
                    Some((_, rid)) => *rid,
                    None => {
                        if url_reqs.len() >= MAX_PAGE_IMAGES {
                            continue; // skip surplus images entirely
                        }
                        let rid = next_req;
                        next_req += 1;
                        url_reqs.push((key, rid));
                        let _ = ctx.current.tables.imagereq().insert(ImageReq {
                            req_id: rid,
                            url: url.clone(),
                        });
                        let _ = ctx
                            .network()
                            .reducers
                            .http_get(rid, loc.host, loc.path);
                        rid
                    }
                };
                ui::create_element(ctx, image_placeholder_el(id.clone(), i as u32));
                let _ = ctx.current.tables.imagewaiter().insert(ImageWaiter {
                    element_id: id,
                    req_id,
                });
            }
        }
    }

    nav.next_req_id = next_req;
    // Consume the main request: the runtime may deliver the same insert to this
    // subscription more than once (we re-enter the broker via http_get mid-run),
    // and clearing main_req_id makes any duplicate delivery a no-op instead of a
    // full wasteful re-render + re-fetch of every image. (Image responses already
    // self-dedupe by deleting their ImageReq row on first delivery.)
    nav.main_req_id = 0;
    let _ = ctx.current.tables.navstate().update(nav.clone());
}

/// Handle a completed image fetch for `req_id`: decode it once into a shared
/// texture, then attach that texture to every element that was waiting on it. If
/// the fetch errored or the bytes don't decode, just drop the waiters (the
/// placeholder stays). Either way the `ImageWaiter` rows for this request are
/// cleared.
fn place_image<Caps>(ctx: &ReducerContext<Caps>, req_id: u64, bytes: &[u8], ok: bool)
where
    Caps: CanRead<ui::UiElement>
        + CanUpdate<ui::UiElement>
        + CanRead<ImageWaiter>
        + CanDelete<ImageWaiter>,
{
    // Which elements were waiting on this fetch.
    let waiters: Vec<String> = ctx
        .current
        .tables
        .imagewaiter()
        .scan()
        .into_iter()
        .filter(|w| w.req_id == req_id)
        .map(|w| w.element_id)
        .collect();
    for id in &waiters {
        let _ = ctx.current.tables.imagewaiter().delete(id.clone());
    }

    let decoded = if ok { image::load_from_memory(bytes).ok() } else { None };
    let Some(img) = decoded else {
        return;
    };
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    if w == 0 || h == 0 {
        return;
    }

    // One texture per distinct fetch, shared by every waiter.
    let tex_id = format!("imgtex_{req_id}");
    let _ = ctx.graphics().reducers.create_texture(
        tex_id.clone(),
        TextureDescriptorInput {
            width: w,
            height: h,
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
        rgba.into_raw(),
    );

    let (dw, dh) = display_size(w, h);
    for id in &waiters {
        // Only attach if the element still exists (the user may have navigated away).
        if let Some(mut el) = ctx.current.tables.uielement().get(id.clone()) {
            el.width = Size::Fixed(dw);
            el.height = Size::Fixed(dh);
            el.background_color = TRANSPARENT;
            el.image = Some(tex_id.clone());
            let _ = ctx.current.tables.uielement().update(el);
        }
    }
}

// ── Per-frame input + render ─────────────────────────────────────────────────

#[reducer(on = "graphics.frametick.update")]
pub fn on_frame<Caps>(ctx: ReducerContext<Caps>, _prev: FrameTick, _tick: FrameTick)
where
    Caps: CanRead<NavState>
        + CanUpdate<NavState>
        + CanRead<ui::UiElement>
        + CanUpdate<ui::UiElement>
        + CanRead<ui::InputFocus>
        + CanRead<LinkMap>
        + CanRead<MouseState>
        + CanRead<MouseButton>
        + CanRead<KeyState>,
{
    let Some(mut nav) = ctx.current.tables.navstate().get(0) else {
        ui::render(&ctx);
        return;
    };

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
    let enter_down = ctx.input().tables.keystate().scan().iter().any(|k| {
        k.pressed && (k.code == KeyCode::Enter as u32 || k.code == KeyCode::NumpadEnter as u32)
    });

    let left_edge = left_down && !nav.prev_left;
    let enter_edge = enter_down && !nav.prev_enter;
    nav.prev_left = left_down;
    nav.prev_enter = enter_down;

    // Click a link → resolve + navigate, and reflect the new URL in the bar.
    if left_edge {
        if let Some(id) = ui::element_at(&ctx, (mx, my)) {
            if let Some(link) = ctx.current.tables.linkmap().get(id) {
                if let Some(loc) = url::resolve(&nav.host, &nav.path, &link.href) {
                    start_navigation(&ctx, &mut nav, &loc.to_url());
                    if let Some(mut bar) = ctx.current.tables.uielement().get(URLBAR_ID.to_string())
                    {
                        bar.text = Some(nav.url.clone());
                        bar.cursor_pos = nav.url.chars().count() as u32;
                        let _ = ctx.current.tables.uielement().update(bar);
                    }
                }
            }
        }
    }

    // Enter → navigate to whatever is typed in the URL bar.
    if enter_edge {
        if let Some(bar) = ctx.current.tables.uielement().get(URLBAR_ID.to_string()) {
            if let Some(text) = bar.text.clone() {
                start_navigation(&ctx, &mut nav, &text);
            }
        }
    }

    let _ = ctx.current.tables.navstate().update(nav);

    // Lay out + draw (also handles wheel scrolling of the viewport).
    ui::render(&ctx);
}

// ── Element builders ─────────────────────────────────────────────────────────

fn text_el(
    id: String,
    order: u32,
    text: String,
    size: f32,
    color: (f32, f32, f32, f32),
    indent: f32,
) -> UiElement {
    UiElement {
        id,
        parent: Some(VIEWPORT_ID.into()),
        order,
        width: Size::Grow,
        height: Size::Fit,
        layout_direction: LayoutDirection::Row,
        gap: 0.0,
        padding: 0.0,
        // Uniform margin approximates a left indent for lists/quotes. Precise
        // per-side insets are out of scope for v1 (see the plan).
        margin: indent,
        background_color: TRANSPARENT,
        corner_radius: 0.0,
        border_width: 0.0,
        border_color: TRANSPARENT,
        text: Some(text),
        text_size: size,
        text_color: color,
        text_wrap: TextWrap::Words,
        image: None,
        is_input: false,
        cursor_pos: 0,
        scrollable_x: false,
        scrollable_y: false,
        scroll_x: 0.0,
        scroll_y: 0.0,
        visible: true,
    }
}

fn space_el(id: String, order: u32, height: f32) -> UiElement {
    UiElement {
        id,
        parent: Some(VIEWPORT_ID.into()),
        order,
        width: Size::Grow,
        height: Size::Fixed(height),
        layout_direction: LayoutDirection::Row,
        gap: 0.0,
        padding: 0.0,
        margin: 0.0,
        background_color: TRANSPARENT,
        corner_radius: 0.0,
        border_width: 0.0,
        border_color: TRANSPARENT,
        text: None,
        text_size: 0.0,
        text_color: TRANSPARENT,
        text_wrap: TextWrap::None,
        image: None,
        is_input: false,
        cursor_pos: 0,
        scrollable_x: false,
        scrollable_y: false,
        scroll_x: 0.0,
        scroll_y: 0.0,
        visible: true,
    }
}

fn image_placeholder_el(id: String, order: u32) -> UiElement {
    UiElement {
        id,
        parent: Some(VIEWPORT_ID.into()),
        order,
        width: Size::Fixed(220.0),
        height: Size::Fixed(140.0),
        layout_direction: LayoutDirection::Row,
        gap: 0.0,
        padding: 0.0,
        margin: 6.0,
        background_color: IMG_PLACEHOLDER_BG,
        corner_radius: 4.0,
        border_width: 0.0,
        border_color: TRANSPARENT,
        text: None,
        text_size: 0.0,
        text_color: TRANSPARENT,
        text_wrap: TextWrap::None,
        image: None,
        is_input: false,
        cursor_pos: 0,
        scrollable_x: false,
        scrollable_y: false,
        scroll_x: 0.0,
        scroll_y: 0.0,
        visible: true,
    }
}

/// Cap image display width so large images don't blow out the layout; height
/// scales proportionally.
fn display_size(w: u32, h: u32) -> (f32, f32) {
    const MAX_W: f32 = 600.0;
    let (wf, hf) = (w as f32, h as f32);
    if wf <= MAX_W {
        (wf, hf)
    } else {
        (MAX_W, hf * (MAX_W / wf))
    }
}
