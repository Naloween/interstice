//! Module-local state tables and the small user-agent palette / element-id
//! constants shared across the browser's submodules. The `#[table]` structs here
//! are the browser's entire persistent state; everything else (parsed pages,
//! cascades) is recomputed each navigation.

use interstice_sdk::*;

/// Our schema name — the broker stamps `HttpResponse.owner` with the caller, so we
/// ignore responses belonging to other apps.
pub const ME: &str = "browser-example";

pub const ROOT_BG: (f32, f32, f32, f32) = (0.08, 0.08, 0.10, 1.0);
pub const BAR_BG: (f32, f32, f32, f32) = (0.16, 0.16, 0.20, 1.0);
pub const BAR_TEXT: (f32, f32, f32, f32) = (0.92, 0.92, 0.95, 1.0);
pub const VIEWPORT_BG: (f32, f32, f32, f32) = (0.11, 0.11, 0.14, 1.0);
pub const TRANSPARENT: (f32, f32, f32, f32) = (0.0, 0.0, 0.0, 0.0);
pub const IMG_PLACEHOLDER_BG: (f32, f32, f32, f32) = (0.16, 0.16, 0.20, 1.0);
pub const BTN_BG: (f32, f32, f32, f32) = (0.16, 0.16, 0.20, 1.0);
pub const BTN_BORDER: (f32, f32, f32, f32) = (0.30, 0.30, 0.36, 1.0);
/// Label colour when the button is actionable…
pub const BTN_TEXT: (f32, f32, f32, f32) = (0.92, 0.92, 0.95, 1.0);
/// …and when it isn't (no history in that direction).
pub const BTN_TEXT_DIM: (f32, f32, f32, f32) = (0.38, 0.38, 0.44, 1.0);

/// Table grid-line colour (revealed through 1px gaps between cells/rows).
pub const TABLE_BORDER: (f32, f32, f32, f32) = (0.30, 0.30, 0.36, 1.0);
/// Body cell background (sits just above the viewport so grid lines read).
pub const TABLE_CELL_BG: (f32, f32, f32, f32) = (0.13, 0.13, 0.17, 1.0);
/// Header (`<th>`) cell background — a touch lighter than body cells.
pub const TABLE_HEADER_BG: (f32, f32, f32, f32) = (0.19, 0.19, 0.24, 1.0);

pub const TOOLBAR_ID: &str = "toolbar";
pub const BACK_BTN_ID: &str = "btn_back";
pub const FWD_BTN_ID: &str = "btn_fwd";

pub const URLBAR_ID: &str = "urlbar";
pub const VIEWPORT_ID: &str = "viewport";

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
    /// Scheme of the current page: true = https (the broker terminates TLS
    /// host-side). Relative links/images inherit it.
    pub tls: bool,
    pub main_req_id: u64,
    pub next_req_id: u64,
    pub nav_gen: u32,
    pub prev_left: bool,
    pub prev_enter: bool,
    /// Back/forward stack. `hist_len` is how many entries exist (in the `History`
    /// table, keyed 0..hist_len); `hist_pos` is the index of the page currently
    /// shown. Back decrements, forward increments, and a fresh navigation (typing
    /// or clicking a link) truncates everything after `hist_pos` and appends.
    pub hist_len: u32,
    pub hist_pos: u32,
    /// Number of external stylesheets still being fetched for the current page.
    /// CSS is render-blocking: the page is rebuilt only once `pending_css` hits 0,
    /// so content is laid out once (images aren't re-fetched on a later re-render).
    pub pending_css: u32,
    /// Identity of the page currently being assembled, bumped once per main-page
    /// load. Stylesheet rows + the stored document are tagged with it so a late
    /// stylesheet response for a page the user already navigated away from is
    /// ignored.
    pub page_gen: u32,
}

/// One entry in the back/forward stack, keyed by its position `idx` (0-based).
/// Stores the URL as the user navigated to it (pre-redirect), so revisiting it
/// re-runs any redirect — simple and good enough.
#[table]
pub struct History {
    #[primary_key]
    pub idx: u32,
    pub url: String,
    pub host: String,
    pub path: String,
    pub tls: bool,
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

/// A queued image fetch, drained by `on_frame`. CRITICAL: image fetches must NOT
/// be issued from inside `render_page` — that runs inside `on_http`, a
/// `network.httpresponse.insert` subscription reducer. Calling `network.http_get`
/// there re-enters the broker mid-run, which makes the runtime RE-DELIVER the
/// triggering response insert → the page renders twice and fires every image
/// fetch twice with the same `req_id`, and two concurrent same-id jobs collide in
/// the broker and corrupt each other's bodies. So render only *queues* fetches
/// here; `on_frame` (driven by the graphics tick, not a network subscription)
/// drains the queue and issues the actual `http_get` outside any re-entrant path.
#[table]
pub struct ImageFetchQueue {
    #[primary_key]
    pub req_id: u64,
    pub host: String,
    pub path: String,
    pub tls: bool,
}

/// The current page's source HTML, stashed so the page can be re-parsed once its
/// render-blocking stylesheets arrive (the first parse only collects stylesheet
/// refs; the real layout happens in `rebuild`). Singleton (`id` always 0).
#[table]
pub struct PageDoc {
    #[primary_key]
    pub id: u32,
    pub html: String,
    pub host: String,
    pub path: String,
    pub tls: bool,
    pub page_gen: u32,
}

/// One stylesheet contributing to the current page's cascade. External sheets
/// start `ready = false` with empty `css` and are filled in when their fetch
/// completes; inline `<style>` sheets are stored `ready = true`. `order` gives the
/// source order within the cascade (externals before inline `<style>`).
#[table]
pub struct StyleSheet {
    #[primary_key]
    pub req_id: u64,
    pub page_gen: u32,
    pub order: u32,
    pub css: String,
    pub ready: bool,
}

/// A queued external stylesheet fetch, drained by `on_frame` (same re-entrancy
/// rule as [`ImageFetchQueue`] — never call the broker from inside `on_http`).
#[table]
pub struct CssFetchQueue {
    #[primary_key]
    pub req_id: u64,
    pub host: String,
    pub path: String,
    pub tls: bool,
}
