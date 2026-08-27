//! The page's JavaScript runtime, built on `boa_engine` (the only pure-Rust JS
//! engine that compiles to wasm32-unknown-unknown — the same constraint that
//! forced Boa over V8/QuickJS). A single Boa [`Context`] lives in a `thread_local`
//! and is rebuilt from scratch on every navigation ([`reset`]), because linear
//! memory persists across reducer calls and the module is single-threaded, so the
//! `thread_local` is both durable and contention-free.
//!
//! Phase 2 scope: a persistent `Context`, a `console` object whose methods forward
//! to the host log, and execution of inline `<script>` elements (no `src`) in
//! document order after the DOM arena is built. DOM bindings, events, timers, and
//! `fetch` arrive in later phases; today scripts run against a bare global.

pub(crate) use boa_engine::{
    context::time::{Clock, JsInstant},
    js_string,
    native_function::NativeFunctionPointer,
    object::{
        builtins::{JsArray, JsFunction, JsPromise},
        ObjectInitializer,
    },
    property::Attribute,
    Context, JsError, JsNativeError, JsObject, JsResult, JsValue, NativeFunction, Source,
};
pub(crate) use std::cell::{Cell, RefCell};
pub(crate) use std::rc::Rc;

pub(crate) use crate::dom::{Dom, NodeId};

// Boa pulls `rand` → `getrandom` 0.3 for `Math.random` and internal seeding. On
// wasm32-unknown-unknown getrandom selects no backend; the workspace sets
// `--cfg getrandom_backend="custom"` (root .cargo/config.toml) which routes every
// entropy request to the symbol below. There is no real entropy source inside the
// module sandbox, so we serve a deterministic splitmix64 stream. That is NOT
// cryptographically secure — acceptable because nothing here is security-sensitive
// (it only feeds Math.random / hash seeds). A host entropy authority can replace
// this later if a page ever needs real randomness.
thread_local! {
    static RNG_STATE: Cell<u64> = const { Cell::new(0x9E3779B97F4A7C15) };
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
            // splitmix64
            x = x.wrapping_add(0x9E3779B97F4A7C15);
            let mut z = x;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
            z ^= z >> 31;
            let bytes = z.to_le_bytes();
            let n = (len - i).min(8);
            // SAFETY: caller guarantees `dest` is valid for `len` bytes; i+n <= len.
            unsafe {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), dest.add(i), n);
            }
            i += n;
        }
        s.set(x);
    });
    Ok(())
}

thread_local! {
    /// The current document's JS realm. `None` until the first navigation; replaced
    /// wholesale by [`reset`] on each new page so one page's globals never leak into
    /// the next. Held as an `Option` so we can build it lazily and drop the old one.
    static JS: RefCell<Option<Context>> = const { RefCell::new(None) };

    /// Registered event listeners for the current page, in registration order.
    /// Keyed by (arena node, lowercased event type); the `handler` is the JS callback
    /// held as an owned GC handle (which keeps it rooted so it survives collection
    /// between the script that registered it and the later event). Cleared by
    /// [`reset`] on each navigation. See [`dispatch_event`].
    static LISTENERS: RefCell<Vec<Listener>> = const { RefCell::new(Vec::new()) };

    /// Per-dispatch flags set by the current Event object's `stopPropagation` /
    /// `preventDefault`. Reset at the start of each [`dispatch_event`] and read back
    /// by it to stop bubbling / report whether the default action was suppressed.
    static EVT_STOP: Cell<bool> = const { Cell::new(false) };
    static EVT_PREVENTED: Cell<bool> = const { Cell::new(false) };

    /// Pending `setTimeout`/`setInterval` callbacks for the current page, each with
    /// the frame it is next due on (see [`FRAME`]). Fired and (for one-shots) removed
    /// by [`pump_frame`]. Callbacks are owned GC handles, so they stay rooted between
    /// the script that scheduled them and the frame that fires them. Cleared by
    /// [`reset`].
    static TIMERS: RefCell<Vec<Timer>> = const { RefCell::new(Vec::new()) };
    static NEXT_TIMER_ID: Cell<u32> = const { Cell::new(1) };

    /// Pending `requestAnimationFrame` callbacks. Unlike timers these are drained in
    /// full on the very next [`pump_frame`] (rAF fires once, next frame). Cleared by
    /// [`reset`].
    static RAF_CBS: RefCell<Vec<(u32, JsValue)>> = const { RefCell::new(Vec::new()) };
    static NEXT_RAF_ID: Cell<u32> = const { Cell::new(1) };

    /// The current frame counter (from `FrameTick.frame`, the module's only clock).
    /// Updated by [`pump_frame`]; read when scheduling a timer to compute its due
    /// frame. Reset to 0 by [`reset`] so each page's timers start from a clean epoch.
    static FRAME: Cell<u64> = const { Cell::new(0) };

    /// The arena node that `document`/`window` event listeners attach to (the `<html>`
    /// root). Document/window-level `addEventListener("click", …)` handlers register
    /// here; since [`dispatch_event`] bubbles a target up to the root, a click on any
    /// element reaches these. `None` until [`install_document`] finds the root.
    static DOC_NODE: Cell<Option<NodeId>> = const { Cell::new(None) };

    /// Backing store for `localStorage`/`sessionStorage`. In-memory only (per page
    /// realm, cleared on navigation via [`reset`]) — portal code tolerates absent
    /// values, so we don't persist.
    static STORAGE: RefCell<std::collections::HashMap<String, String>> =
        RefCell::new(std::collections::HashMap::new());
}

/// One registered `addEventListener` entry.
struct Listener {
    node: NodeId,
    event: String,
    handler: JsValue,
}

/// One pending `setTimeout` (`interval_frames` = `None`) or `setInterval`
/// (`Some(period)`) callback. `args` are the extra arguments passed after the delay,
/// forwarded to the callback per the HTML timer spec.
struct Timer {
    id: u32,
    callback: JsValue,
    args: Vec<JsValue>,
    due_frame: u64,
    interval_frames: Option<u64>,
}

/// The clock Boa reads for `Date.now()` and, crucially, for ordering the promise
/// job queue. The default `StdClock` calls `SystemTime::now`, which panics on
/// wasm32-unknown-unknown (no OS clock) — so we derive time from the frame counter
/// ([`FRAME`]) instead: monotonic, epoch-relative, and never touching `std::time`'s
/// unsupported now(). ~16ms/frame matches [`ms_to_frames`].
#[derive(Debug)]
struct FrameClock;

impl Clock for FrameClock {
    fn now(&self) -> JsInstant {
        let ms = FRAME.with(|f| f.get()).saturating_mul(16);
        JsInstant::new(ms / 1000, ((ms % 1000) * 1_000_000) as u32)
    }
}

/// Approximate a millisecond delay as a whole number of frames, assuming ~60fps
/// (16ms/frame) — the module has no wall clock, only the monotonic frame counter.
/// Always at least 1 so `setTimeout(fn, 0)` fires next frame rather than re-entrantly.
fn ms_to_frames(ms: f64) -> u64 {
    if !ms.is_finite() || ms <= 0.0 {
        return 1;
    }
    ((ms / 16.0).round() as u64).max(1)
}

/// Start a fresh JS realm for a new page: build a new Boa [`Context`], install the
/// `console` object, and store it as the current realm (dropping the previous
/// page's). Call once per navigation, before running any of the page's scripts.
pub(crate) fn reset() {
    LISTENERS.with(|l| l.borrow_mut().clear());
    TIMERS.with(|t| t.borrow_mut().clear());
    RAF_CBS.with(|r| r.borrow_mut().clear());
    PENDING_NAV.with(|p| p.borrow_mut().clear());
    FETCH_QUEUE.with(|q| q.borrow_mut().clear());
    PENDING_FETCH.with(|p| p.borrow_mut().clear());
    NEXT_FETCH_ID.with(|n| n.set(FETCH_ID_BASE));
    FRAME.with(|f| f.set(0));
    STORAGE.with(|s| s.borrow_mut().clear());
    DOC_NODE.with(|c| c.set(None));
    // Build via the builder (not `Context::default`) so we can install a frame-based
    // clock; the default clock panics on wasm (see [`FrameClock`]). `.build()` still
    // installs the full standard library, same as `default`.
    let mut ctx = Context::builder()
        .clock(Rc::new(FrameClock))
        .build()
        .expect("boa context build is infallible with default settings");
    // Watchdog: cap the loop back-edges any single call frame may take before Boa
    // throws. The count resets per call frame, so a long-lived page doing bounded
    // work each frame is unaffected — only one runaway `while(true)`-style loop is
    // caught, turning a frame-reducer hang into a contained, logged JS error.
    ctx.runtime_limits_mut()
        .set_loop_iteration_limit(50_000_000);
    install_console(&mut ctx);
    install_document(&mut ctx);
    install_window(&mut ctx);
    install_dom_constructors(&mut ctx);
    install_navigator(&mut ctx);
    install_storage(&mut ctx);
    install_image(&mut ctx);
    install_timers(&mut ctx);
    install_location(&mut ctx);
    install_history(&mut ctx);
    install_fetch(&mut ctx);
    install_xhr(&mut ctx);
    JS.with(|c| *c.borrow_mut() = Some(ctx));
}

mod scripts;
mod element;
mod events;
mod window;
mod location;
mod net;
mod document;
mod selector;

pub(crate) use scripts::*;
pub(crate) use element::*;
pub(crate) use events::*;
pub(crate) use window::*;
pub(crate) use location::*;
pub(crate) use net::*;
pub(crate) use document::*;
pub(crate) use selector::*;

