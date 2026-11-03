//! The browser's state, as tables. This is the whole truth: everything else in
//! module memory (parsed documents, the chrome document) is an
//! [`InstanceCache`](interstice_sdk::InstanceCache) derived from these rows,
//! rebuilt on any instance that needs it (see [`crate::cache`]).
//!
//! | table          | persistence | role                                             |
//! |----------------|-------------|--------------------------------------------------|
//! | `Session`      | stateful    | singleton: active tab, id counters, homepage     |
//! | `Tab`          | stateful    | open tabs + their history (restored on restart)  |
//! | `Ui`           | ephemeral   | singleton: pointer, focus, URL field, textures   |
//! | `Document`     | ephemeral   | the HTML a tab currently shows                   |
//! | `DocEvent`     | ephemeral   | input applied to a document, in order (replayed  |
//! |                |             | when another instance rebuilds the document)     |
//! | `PendingInput`   | ephemeral   | raw key/button input queued for `on_frame`       |
//! | `Request`      | ephemeral   | HTTP requests in flight (or waiting to be sent)  |
//! | `Resource`     | ephemeral   | sub-resource cache (url → bytes), FIFO-bounded   |
//! | `ResourceMeta` | ephemeral   | size + age of each `Resource`, for eviction      |
//! | `ScriptedInput`| ephemeral   | headless test input (`browser_script` file)      |

use interstice_sdk::*;

pub const SESSION: u32 = 0;
pub const UI: u32 = 0;

/// Sub-resource cache budget.
pub const RESOURCE_BUDGET: u64 = 64 * 1024 * 1024;

#[table(stateful)]
#[derive(Debug, PartialEq, Default)]
pub struct Session {
    #[primary_key]
    pub id: u32,
    pub active_tab: u64,
    pub next_tab: u64,
    pub next_doc: u64,
    pub next_req: u64,
    pub home: String,
    /// Bumped whenever a `Resource` arrives, so documents waiting on
    /// sub-resources only look them up when something new is there.
    pub res_rev: u64,
    /// Total size of the `Resource` table, and the insertion clock used as
    /// its eviction order.
    pub res_bytes: u64,
    pub res_clock: u64,
}

#[table(stateful)]
#[derive(Debug, PartialEq)]
pub struct Tab {
    #[primary_key]
    pub id: u64,
    pub url: String,
    pub title: String,
    pub favicon: Option<String>,
    pub history: Vec<String>,
    pub hist_pos: u32,
    /// Main-frame request in flight, if any.
    pub loading: Option<u64>,
    /// `Document` shown by this tab (0 = none yet).
    pub doc: u64,
    /// `seq` of the last `DocEvent` applied to that document.
    pub doc_seq: u64,
    /// Title/favicon have been extracted from that document.
    pub doc_parsed: bool,
}

#[table(ephemeral)]
#[derive(Debug, PartialEq, Default)]
pub struct Ui {
    #[primary_key]
    pub id: u32,
    /// Our own clamped pointer, integrated from the raw mouse position.
    pub mouse: (f32, f32),
    pub last_raw: Option<(f32, f32)>,
    pub last_wheel: Option<(f32, f32)>,
    /// `keys::Mods` and `MouseEventButtons` bits.
    pub mods: u8,
    pub buttons: u8,
    /// 0 none, 1 chrome, 2 content.
    pub region: u8,
    pub focus_chrome: bool,
    pub url_editing: bool,
    pub url_text: String,
    pub viewport: (u32, u32),
    /// GPU textures we own in the graphics module (empty id = none).
    pub chrome_tex: String,
    pub chrome_tex_size: (u32, u32),
    pub content_tex: String,
    pub content_tex_size: (u32, u32),
    /// Cursor atlas (`cursor.rs`), created on first draw.
    pub cursor_tex: String,
    pub tex_gen: u64,
    /// Identity of what each texture currently shows: the `stamp` of the
    /// cache that painted it. A different stamp (another instance's cache, a
    /// different document) means repaint.
    pub chrome_stamp: u64,
    pub content_stamp: u64,
    pub next_stamp: u64,
}

#[table(ephemeral)]
#[derive(Debug, PartialEq)]
pub struct Document {
    #[primary_key]
    pub id: u64,
    pub tab: u64,
    pub url: String,
    /// Immutable once inserted (progress lives in the `Tab` row), so the large
    /// row is never rewritten.
    pub html: String,
    /// Wall-clock ms when the document was committed: the origin of its
    /// script clock (`performance.now()`), so timers replay identically.
    pub t0: f64,
}

pub const EV_DOWN: u8 = 0;
pub const EV_UP: u8 = 1;
pub const EV_KEY_DOWN: u8 = 2;
pub const EV_KEY_UP: u8 = 3;
pub const EV_WHEEL: u8 = 4;
/// Keyboard scrolling (no editable control focused): px, positive = down/right.
pub const EV_SCROLL: u8 = 5;
/// Pointer motion — logged only when page scripts listen for motion events.
pub const EV_MOVE: u8 = 6;
/// Script clock advance: run timers / animation frames due at `code` ms
/// (relative to `Document::t0`).
pub const EV_TICK: u8 = 7;
/// A script `fetch` (id `code`) completed; its body is the `Resource` for
/// the URL the script asked for (missing / empty = network error).
pub const EV_FETCH: u8 = 8;
/// Compaction snapshot: absolute viewport scroll `(dx, dy)`. A document whose
/// log outgrows `engine::MAX_DOC_EVENTS` has it replaced by this single event,
/// so a rebuild behaves like a reloaded discarded tab (scroll kept, form and
/// script state reset).
pub const EV_SCROLL_TO: u8 = 9;

/// One input event delivered to a content document. Replaying a document's
/// events in `seq` order over its HTML reproduces its interactive state
/// (scroll position, form contents, focus, `<details>` …).
#[table(ephemeral)]
#[derive(Debug, PartialEq)]
pub struct DocEvent {
    #[primary_key(auto_inc)]
    pub seq: u64,
    #[index(hash)]
    pub doc: u64,
    pub kind: u8,
    pub x: f32,
    pub y: f32,
    pub button: u8,
    pub buttons: u8,
    pub mods: u8,
    pub code: u32,
    pub repeat: bool,
    pub dx: f32,
    pub dy: f32,
    /// Key events: the layout's logical key and typed text ("" = unknown).
    pub key: String,
    pub text: String,
}

pub const IN_KEY: u8 = 0;
pub const IN_BUTTON: u8 = 1;

#[table(ephemeral)]
#[derive(Debug, PartialEq)]
pub struct PendingInput {
    #[primary_key(auto_inc)]
    pub seq: u64,
    pub kind: u8,
    pub code: u32,
    pub pressed: bool,
    pub repeat: bool,
    /// Keys: the input module's logical key and typed text ("" = unknown).
    pub key: String,
    pub text: String,
}

pub const REQ_MAIN: u8 = 0;
pub const REQ_SUB: u8 = 1;
pub const REQ_FAVICON: u8 = 2;

pub const NAV_PUSH: u8 = 0;
pub const NAV_REPLACE: u8 = 1;
pub const NAV_TRAVERSE: u8 = 2;

#[table(ephemeral)]
#[derive(Debug, PartialEq)]
pub struct Request {
    #[primary_key]
    pub req_id: u64,
    pub kind: u8,
    #[index(hash)]
    pub url: String,
    /// Main / favicon: the tab it belongs to.
    pub tab: u64,
    /// Main: how the navigation affects history (`NAV_*`, + index).
    pub nav_mode: u8,
    pub nav_index: u32,
    /// Handed to the network broker yet.
    pub sent: bool,
}

#[table(ephemeral)]
pub struct Resource {
    #[primary_key]
    pub url: String,
    pub body: Vec<u8>,
}

#[table(ephemeral)]
pub struct ResourceMeta {
    #[primary_key]
    pub url: String,
    pub size: u64,
    pub stamp: u64,
}

/// Headless test input from the `browser_script` file: `command` runs on
/// frame `frame` (see `Browser::script` for the commands).
#[table(ephemeral)]
pub struct ScriptedInput {
    #[primary_key(auto_inc)]
    pub id: u64,
    pub frame: u64,
    pub command: String,
}

/// `localStorage`: persists across documents and sessions, per origin.
#[table(stateful)]
#[derive(Debug, PartialEq)]
pub struct WebStorage {
    /// `origin + "\0" + name`.
    #[primary_key]
    pub key: String,
    #[index(hash)]
    pub origin: String,
    pub name: String,
    pub value: String,
}
