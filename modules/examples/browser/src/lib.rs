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
//!
//! The implementation is split across focused submodules:
//! - [`tables`] — module state tables + the UA palette / element-id constants.
//! - [`nav`] — navigation + history primitives.
//! - [`page`] — HTTP-response → parsed-page pipeline (incl. stylesheet gathering).
//! - [`render`] — `Block` tree → `UiElement` materialisation (box model / flex).
//! - [`images`] — image decode (raster + SVG) and texture upload.
//! - [`widgets`] — chrome/content `UiElement` builders + toolbar sync helpers.
//! - [`html`] / [`css`] / [`style`] / [`url`] — parsing, the cascade, and URLs.
//!
//! `lib.rs` itself is just the wiring: the module declaration, the UI subsystem,
//! and the three reducer entry points (`on_load` / `on_http` / `on_frame`), each
//! delegating into the submodules above.

use crate::bindings::{graphics::*, input::*, network::*};
use interstice_sdk::key_code::KeyCode;
use interstice_sdk::*;

mod css;
mod html;
mod images;
mod nav;
mod page;
mod render;
mod style;
mod table;
mod tables;
mod url;
mod widgets;

interstice_module!(visibility: Public);

// Module-local UI subsystem (own tables, helpers, render + key reducer), wired to
// this module's own graphics/input bindings so the compositor can route us to our
// own surface.
interstice_ui::ui_subsystem!();

use crate::tables::*;
use crate::ui::*;

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
            ..Default::default()
        },
    );

    // Toolbar — a row holding the back/forward buttons and the URL bar.
    ui::create_element(
        &ctx,
        UiElement {
            id: TOOLBAR_ID.into(),
            parent: Some("root".into()),
            order: 0,
            width: Size::Grow,
            height: Size::Fixed(42.0),
            layout_direction: LayoutDirection::Row,
            gap: 6.0,
            padding: 6.0,
            margin: 0.0,
            background_color: ROOT_BG,
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
            ..Default::default()
        },
    );
    // Back / forward buttons (start dimmed — no history yet).
    ui::create_element(&ctx, widgets::nav_button_el(BACK_BTN_ID.into(), 0, "<"));
    ui::create_element(&ctx, widgets::nav_button_el(FWD_BTN_ID.into(), 1, ">"));

    // URL bar — an editable text input. Focused below so typing edits it and
    // Enter navigates. Starts empty; the user types a URL to load a page.
    ui::create_element(
        &ctx,
        UiElement {
            id: URLBAR_ID.into(),
            parent: Some(TOOLBAR_ID.into()),
            order: 2,
            width: Size::Grow,
            height: Size::Grow,
            layout_direction: LayoutDirection::Row,
            gap: 0.0,
            padding: 7.0,
            margin: 0.0,
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
            ..Default::default()
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
            ..Default::default()
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
        tls: false,
        main_req_id: 0,
        next_req_id: 1,
        nav_gen: 0,
        prev_left: false,
        prev_enter: false,
        hist_len: 0,
        hist_pos: 0,
        pending_css: 0,
        page_gen: 0,
    };
    if let Err(err) = ctx.current.tables.navstate().insert(nav) {
        ctx.log(&format!("browser: failed to seed NavState: {err}"));
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
        + CanDelete<ImageWaiter>
        + CanInsert<ImageFetchQueue>
        + CanRead<PageDoc>
        + CanInsert<PageDoc>
        + CanUpdate<PageDoc>
        + CanDelete<PageDoc>
        + CanRead<StyleSheet>
        + CanInsert<StyleSheet>
        + CanUpdate<StyleSheet>
        + CanDelete<StyleSheet>
        + CanInsert<CssFetchQueue>,
{
    if row.owner != ME {
        return; // another app's request
    }
    let Some(mut nav) = ctx.current.tables.navstate().get(0) else {
        return;
    };

    if row.req_id == nav.main_req_id {
        page::prepare_page(&ctx, &mut nav, &row);
    } else if ctx.current.tables.stylesheet().get(row.req_id).is_some() {
        page::on_css_response(&ctx, &mut nav, &row);
    } else if ctx.current.tables.imagereq().get(row.req_id).is_some() {
        // An image fetch completed. Drop the request record (so a duplicate
        // delivery is ignored) and hand the decoded texture to every element that
        // was waiting on this URL.
        let _ = ctx.current.tables.imagereq().delete(row.req_id);
        images::place_image(&ctx, row.req_id, &row.body, row.error.is_empty());
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
        + CanRead<History>
        + CanInsert<History>
        + CanDelete<History>
        + CanRead<ImageFetchQueue>
        + CanDelete<ImageFetchQueue>
        + CanRead<CssFetchQueue>
        + CanDelete<CssFetchQueue>
        + CanRead<MouseState>
        + CanRead<MouseButton>
        + CanRead<KeyState>,
{
    // Drain queued fetches OUTSIDE the on_http re-entrant path: prepare_page/rebuild
    // (which run inside the network subscription reducer) only enqueue fetches; we
    // issue the actual http_get here, where re-entering the broker can't make it
    // re-deliver a response and double-fire fetches. See ImageFetchQueue docs.
    for q in ctx.current.tables.cssfetchqueue().scan() {
        let _ = ctx
            .network()
            .reducers
            .http_get(q.req_id, q.host.clone(), q.path.clone(), q.tls);
        let _ = ctx.current.tables.cssfetchqueue().delete(q.req_id);
    }
    for q in ctx.current.tables.imagefetchqueue().scan() {
        let _ = ctx
            .network()
            .reducers
            .http_get(q.req_id, q.host.clone(), q.path.clone(), q.tls);
        let _ = ctx.current.tables.imagefetchqueue().delete(q.req_id);
    }

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

    // A click can hit a toolbar button or a page link. Resolve the target element
    // once, then dispatch.
    if left_edge {
        if let Some(id) = ui::element_at(&ctx, (mx, my)) {
            if id == BACK_BTN_ID && nav.hist_pos > 0 {
                // Back: step to the previous entry and re-fetch it (no history push).
                nav.hist_pos -= 1;
                if let Some(url) = nav::load_current_history(&ctx, &mut nav) {
                    widgets::set_urlbar(&ctx, &url);
                }
            } else if id == FWD_BTN_ID && nav.hist_pos + 1 < nav.hist_len {
                // Forward: step to the next entry and re-fetch it.
                nav.hist_pos += 1;
                if let Some(url) = nav::load_current_history(&ctx, &mut nav) {
                    widgets::set_urlbar(&ctx, &url);
                }
            } else if let Some(href) = ui::link_at(&ctx, (mx, my)) {
                // Click an inline link span → resolve + navigate, reflect the URL.
                if let Some(loc) = url::resolve(&nav.host, &nav.path, nav.tls, &href) {
                    nav::start_navigation(&ctx, &mut nav, &loc.to_url());
                    widgets::set_urlbar(&ctx, &nav.url.clone());
                }
            }
        }
    }

    // Enter → navigate to whatever is typed in the URL bar.
    if enter_edge {
        if let Some(bar) = ctx.current.tables.uielement().get(URLBAR_ID.to_string()) {
            if let Some(text) = bar.text.clone() {
                nav::start_navigation(&ctx, &mut nav, &text);
            }
        }
    }

    // Dim the buttons that lead nowhere so it's clear when back/forward is available.
    widgets::update_button_state(&ctx, BACK_BTN_ID, nav.hist_pos > 0);
    widgets::update_button_state(&ctx, FWD_BTN_ID, nav.hist_pos + 1 < nav.hist_len);

    let _ = ctx.current.tables.navstate().update(nav);

    // Lay out + draw (also handles wheel scrolling of the viewport).
    ui::render(&ctx);
}
