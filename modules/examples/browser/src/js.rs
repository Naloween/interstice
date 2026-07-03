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

use boa_engine::{
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
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::dom::{Dom, NodeId};

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
    install_timers(&mut ctx);
    install_location(&mut ctx);
    install_history(&mut ctx);
    install_fetch(&mut ctx);
    install_xhr(&mut ctx);
    JS.with(|c| *c.borrow_mut() = Some(ctx));
}

/// One `console` method: stringify every argument, join with spaces, and forward
/// the line to the host log prefixed with its level. Shared by log/info/warn/error
/// via a captured `&'static str` level (a `Copy` closure, so `from_copy_closure`
/// is sound — it captures no traceable GC values).
fn console_method(
    level: &'static str,
) -> impl Fn(&JsValue, &[JsValue], &mut Context) -> boa_engine::JsResult<JsValue> + Copy {
    move |_this, args, ctx| {
        let mut line = String::new();
        for (i, arg) in args.iter().enumerate() {
            if i > 0 {
                line.push(' ');
            }
            match arg.to_string(ctx) {
                Ok(s) => line.push_str(&s.to_std_string_escaped()),
                Err(_) => line.push_str("<unprintable>"),
            }
        }
        interstice_sdk::host_calls::log(&format!("js:console.{level}: {line}"));
        Ok(JsValue::undefined())
    }
}

/// Register a `console` global with log/info/warn/error, each forwarding to the
/// host log. Enough for pages to report progress and for us to see what a script
/// does during the headless smoke; richer console methods can follow.
fn install_console(ctx: &mut Context) {
    let console = ObjectInitializer::new(ctx)
        .function(
            NativeFunction::from_copy_closure(console_method("log")),
            js_string!("log"),
            0,
        )
        .function(
            NativeFunction::from_copy_closure(console_method("info")),
            js_string!("info"),
            0,
        )
        .function(
            NativeFunction::from_copy_closure(console_method("warn")),
            js_string!("warn"),
            0,
        )
        .function(
            NativeFunction::from_copy_closure(console_method("error")),
            js_string!("error"),
            0,
        )
        .build();
    // Non-enumerable, like a real host object, but writable/configurable so a page
    // may override it.
    let _ = ctx.register_global_property(
        js_string!("console"),
        console,
        Attribute::WRITABLE | Attribute::CONFIGURABLE,
    );
}

/// Run every inline `<script>` (one without a `src` attribute) in document order,
/// evaluating each in the current realm. External scripts are ignored for now (no
/// fetch wiring yet). A script that throws is logged and does not abort the rest —
/// matching browser behaviour, where one script's error doesn't stop later ones.
/// Call after [`reset`] and after the DOM arena for the page has been built.
pub(crate) fn run_inline_scripts() {
    let scripts = crate::dom::with(collect_inline_scripts);
    if scripts.is_empty() {
        return;
    }
    JS.with(|cell| {
        let mut guard = cell.borrow_mut();
        let Some(ctx) = guard.as_mut() else {
            return;
        };
        for code in &scripts {
            if let Err(e) = ctx.eval(Source::from_bytes(code)) {
                interstice_sdk::host_calls::log(&format!("js: script error: {e}"));
            }
        }
        // Drain any microtasks (resolved Promises, queueMicrotask) the scripts left
        // pending, so `.then` continuations run before we paint.
        if let Err(e) = ctx.run_jobs() {
            interstice_sdk::host_calls::log(&format!("js: microtask error: {e}"));
        }
    });
}

/// Collect the source of every inline `<script>` in document order.
fn collect_inline_scripts(dom: &crate::dom::Dom) -> Vec<String> {
    let mut out = Vec::new();
    for &root in dom.roots() {
        collect_scripts_from(dom, root, &mut out);
    }
    out
}

fn collect_scripts_from(dom: &crate::dom::Dom, id: crate::dom::NodeId, out: &mut Vec<String>) {
    let node = dom.node(id);
    if node.tag() == Some("script") {
        // Skip external scripts (`src`) and non-JS types (e.g. application/json,
        // importmap). An empty/absent type, or an explicit JS mime, is executable.
        let external = node.attr("src").is_some();
        let executable = match node.attr("type") {
            None => true,
            Some(t) => {
                let t = t.trim().to_ascii_lowercase();
                t.is_empty()
                    || t == "text/javascript"
                    || t == "application/javascript"
                    || t == "module"
            }
        };
        if !external && executable {
            let src = dom.inner_text(id);
            if !src.trim().is_empty() {
                out.push(src);
            }
        }
        // `<script>` has no rendered children to recurse into.
        return;
    }
    for &child in &node.children {
        collect_scripts_from(dom, child, out);
    }
}

// ── Read-only DOM bindings (Phase 3) ─────────────────────────────────────────
//
// Scripts get a `document` and element objects that read back into the persistent
// DOM arena. Everything here is READ-ONLY: no property here mutates the tree, so
// there is no re-render to trigger yet (that is Phase 4). An element is an ordinary
// JS object carrying a hidden `__node` number — its arena [`NodeId`] — plus eager
// scalar properties (`tagName`, `textContent`, …) and live method lookups
// (`getAttribute`, `querySelector`, …) that re-read the arena on each call.

/// The hidden data property on an element object holding its arena [`NodeId`].
/// Non-enumerable, non-writable, non-configurable via [`Attribute::empty`], so it
/// never shows up in `for…in`/`Object.keys` and a page can't clobber it.
fn node_key() -> boa_engine::JsString {
    js_string!("__node")
}

/// Read-only + enumerable — the attribute set for an element's scalar DOM props.
fn ro() -> Attribute {
    Attribute::READONLY | Attribute::ENUMERABLE
}

/// Recover the arena node id from a method's `this`, or `None` if `this` isn't an
/// element object we built (e.g. the method was `call`-ed on something else).
fn this_node(this: &JsValue, ctx: &mut Context) -> Option<NodeId> {
    let obj = this.as_object()?;
    let v = obj.get(node_key(), ctx).ok()?;
    Some(v.to_u32(ctx).ok()? as NodeId)
}

/// The `idx`-th argument coerced to a Rust string (empty if absent/uncoercible).
fn arg_string(args: &[JsValue], idx: usize, ctx: &mut Context) -> String {
    args.get(idx)
        .and_then(|v| v.to_string(ctx).ok())
        .map(|s| s.to_std_string_escaped())
        .unwrap_or_default()
}

/// The `idx`-th argument interpreted as one of our element objects → its arena id.
fn arg_node(args: &[JsValue], idx: usize, ctx: &mut Context) -> Option<NodeId> {
    let obj = args.get(idx)?.as_object()?;
    let v = obj.get(node_key(), ctx).ok()?;
    Some(v.to_u32(ctx).ok()? as NodeId)
}

/// Wrap a native fn pointer as a `JsFunction` in `ctx`'s realm — for accessors,
/// which (unlike methods on `ObjectInitializer`) take `JsFunction`s directly.
fn js_fn(ctx: &Context, ptr: NativeFunctionPointer) -> JsFunction {
    NativeFunction::from_fn_ptr(ptr).to_js_function(ctx.realm())
}

/// Build a JS element object wrapping arena node `id`. Structural and textual
/// properties are **live accessors** (they read/write the arena on each access), so
/// a handle stays correct across mutations; identity is the hidden `__node` id.
fn element_object(ctx: &mut Context, id: NodeId) -> JsValue {
    // Accessor functions must be built before the initializer borrows `ctx`.
    let get_tag = js_fn(ctx, el_get_tag_name);
    let get_id = js_fn(ctx, el_get_id);
    let set_id = js_fn(ctx, el_set_id);
    let get_class = js_fn(ctx, el_get_class_name);
    let set_class = js_fn(ctx, el_set_class_name);
    let get_text = js_fn(ctx, el_get_text_content);
    let set_text = js_fn(ctx, el_set_text_content);
    let get_inner = js_fn(ctx, el_get_inner_html);
    let set_inner = js_fn(ctx, el_set_inner_html);
    let get_outer = js_fn(ctx, el_get_outer_html);
    let get_parent = js_fn(ctx, el_get_parent_element);
    let get_children = js_fn(ctx, el_get_children);
    let get_first = js_fn(ctx, el_get_first_element_child);
    let get_next = js_fn(ctx, el_get_next_element_sibling);
    let get_classlist = js_fn(ctx, el_get_class_list);

    let obj = ObjectInitializer::new(ctx)
        .property(node_key(), id as u32, Attribute::empty())
        .accessor(js_string!("tagName"), Some(get_tag.clone()), None, ro())
        .accessor(js_string!("nodeName"), Some(get_tag), None, ro())
        .accessor(js_string!("id"), Some(get_id), Some(set_id), ro())
        .accessor(js_string!("className"), Some(get_class), Some(set_class), ro())
        .accessor(
            js_string!("textContent"),
            Some(get_text.clone()),
            Some(set_text.clone()),
            ro(),
        )
        .accessor(js_string!("innerText"), Some(get_text), Some(set_text), ro())
        .accessor(js_string!("innerHTML"), Some(get_inner), Some(set_inner), ro())
        .accessor(js_string!("outerHTML"), Some(get_outer), None, ro())
        .accessor(js_string!("parentElement"), Some(get_parent), None, ro())
        .accessor(js_string!("children"), Some(get_children), None, ro())
        .accessor(js_string!("firstElementChild"), Some(get_first), None, ro())
        .accessor(js_string!("nextElementSibling"), Some(get_next), None, ro())
        .accessor(js_string!("classList"), Some(get_classlist), None, ro())
        .function(
            NativeFunction::from_fn_ptr(el_get_attribute),
            js_string!("getAttribute"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(el_has_attribute),
            js_string!("hasAttribute"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(el_set_attribute),
            js_string!("setAttribute"),
            2,
        )
        .function(
            NativeFunction::from_fn_ptr(el_remove_attribute),
            js_string!("removeAttribute"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(el_query_selector),
            js_string!("querySelector"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(el_query_selector_all),
            js_string!("querySelectorAll"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(el_get_elements_by_tag_name),
            js_string!("getElementsByTagName"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(el_append_child),
            js_string!("appendChild"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(el_insert_before),
            js_string!("insertBefore"),
            2,
        )
        .function(
            NativeFunction::from_fn_ptr(el_remove_child),
            js_string!("removeChild"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(el_replace_child),
            js_string!("replaceChild"),
            2,
        )
        .function(NativeFunction::from_fn_ptr(el_remove), js_string!("remove"), 0)
        .function(
            NativeFunction::from_fn_ptr(el_add_event_listener),
            js_string!("addEventListener"),
            2,
        )
        .function(
            NativeFunction::from_fn_ptr(el_remove_event_listener),
            js_string!("removeEventListener"),
            2,
        )
        .build();
    obj.into()
}

// Accessor getters/setters. Getters ignore args; setters take the new value as
// `args[0]`. All return `undefined` on a bad `this` rather than throwing.

fn el_get_tag_name(this: &JsValue, _a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::undefined());
    };
    let tag = crate::dom::with(|d| d.node(id).tag().unwrap_or_default().to_ascii_uppercase());
    Ok(js_string!(tag).into())
}

fn el_get_id(this: &JsValue, _a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    Ok(js_string!(attr_of(this, ctx, "id")).into())
}

fn el_set_id(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    set_attr_from(this, args, ctx, "id");
    Ok(JsValue::undefined())
}

fn el_get_class_name(this: &JsValue, _a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    Ok(js_string!(attr_of(this, ctx, "class")).into())
}

fn el_set_class_name(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    set_attr_from(this, args, ctx, "class");
    Ok(JsValue::undefined())
}

fn el_get_text_content(this: &JsValue, _a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::undefined());
    };
    let text = crate::dom::with(|d| d.inner_text(id));
    Ok(js_string!(text).into())
}

fn el_set_text_content(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::undefined());
    };
    let text = arg_string(args, 0, ctx);
    crate::dom::with_mut(|d| d.set_text_content(id, &text));
    Ok(JsValue::undefined())
}

fn el_get_inner_html(this: &JsValue, _a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::undefined());
    };
    let html = crate::dom::with(|d| {
        let mut s = String::new();
        for &c in &d.node(id).children {
            s.push_str(&d.serialize(c));
        }
        s
    });
    Ok(js_string!(html).into())
}

fn el_set_inner_html(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::undefined());
    };
    let html = arg_string(args, 0, ctx);
    crate::dom::with_mut(|d| d.set_inner_html(id, &html));
    Ok(JsValue::undefined())
}

fn el_get_outer_html(this: &JsValue, _a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::undefined());
    };
    let html = crate::dom::with(|d| d.serialize(id));
    Ok(js_string!(html).into())
}

fn el_get_parent_element(this: &JsValue, _a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::null());
    };
    // Return the parent only if it is itself an element (matches parentElement).
    let parent = crate::dom::with(|d| {
        d.node(id)
            .parent
            .filter(|&p| d.node(p).tag().is_some())
    });
    Ok(match parent {
        Some(pid) => element_object(ctx, pid),
        None => JsValue::null(),
    })
}

fn el_get_children(this: &JsValue, _a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsArray::new(ctx).into());
    };
    let kids = crate::dom::with(|d| {
        d.node(id)
            .children
            .iter()
            .copied()
            .filter(|&c| d.node(c).tag().is_some())
            .collect::<Vec<_>>()
    });
    let arr = JsArray::new(ctx);
    for c in kids {
        let el = element_object(ctx, c);
        arr.push(el, ctx)?;
    }
    Ok(arr.into())
}

fn el_get_first_element_child(
    this: &JsValue,
    _a: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::null());
    };
    let first = crate::dom::with(|d| {
        d.node(id)
            .children
            .iter()
            .copied()
            .find(|&c| d.node(c).tag().is_some())
    });
    Ok(match first {
        Some(c) => element_object(ctx, c),
        None => JsValue::null(),
    })
}

fn el_get_next_element_sibling(
    this: &JsValue,
    _a: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::null());
    };
    let sib = crate::dom::with(|d| {
        let parent = d.node(id).parent?;
        let sibs = &d.node(parent).children;
        let pos = sibs.iter().position(|&c| c == id)?;
        sibs[pos + 1..]
            .iter()
            .copied()
            .find(|&c| d.node(c).tag().is_some())
    });
    Ok(match sib {
        Some(c) => element_object(ctx, c),
        None => JsValue::null(),
    })
}

fn el_set_attribute(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::undefined());
    };
    let name = arg_string(args, 0, ctx);
    let value = arg_string(args, 1, ctx);
    crate::dom::with_mut(|d| d.set_attr(id, &name, &value));
    Ok(JsValue::undefined())
}

fn el_remove_attribute(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::undefined());
    };
    let name = arg_string(args, 0, ctx);
    crate::dom::with_mut(|d| d.remove_attr(id, &name));
    Ok(JsValue::undefined())
}

fn el_append_child(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let (Some(parent), Some(child)) = (this_node(this, ctx), arg_node(args, 0, ctx)) else {
        return Ok(JsValue::undefined());
    };
    crate::dom::with_mut(|d| d.append_child(parent, child));
    Ok(args.first().cloned().unwrap_or_default())
}

fn el_insert_before(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let (Some(parent), Some(child)) = (this_node(this, ctx), arg_node(args, 0, ctx)) else {
        return Ok(JsValue::undefined());
    };
    let reference = arg_node(args, 1, ctx);
    crate::dom::with_mut(|d| d.insert_before(parent, child, reference));
    Ok(args.first().cloned().unwrap_or_default())
}

fn el_remove_child(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let (Some(parent), Some(child)) = (this_node(this, ctx), arg_node(args, 0, ctx)) else {
        return Ok(JsValue::undefined());
    };
    crate::dom::with_mut(|d| d.remove_child(parent, child));
    Ok(args.first().cloned().unwrap_or_default())
}

fn el_replace_child(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    // replaceChild(newChild, oldChild): put newChild where oldChild was, drop old.
    let (Some(parent), Some(new_child), Some(old_child)) = (
        this_node(this, ctx),
        arg_node(args, 0, ctx),
        arg_node(args, 1, ctx),
    ) else {
        return Ok(JsValue::undefined());
    };
    crate::dom::with_mut(|d| {
        d.insert_before(parent, new_child, Some(old_child));
        d.remove_child(parent, old_child);
    });
    Ok(args.get(1).cloned().unwrap_or_default())
}

fn el_remove(this: &JsValue, _a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::undefined());
    };
    crate::dom::with_mut(|d| {
        if let Some(p) = d.node(id).parent {
            d.remove_child(p, id);
        }
    });
    Ok(JsValue::undefined())
}

// ── Events (Phase 5) ─────────────────────────────────────────────────────────
//
// `addEventListener` records a (node, type, handler) row; a click resolved to a
// DOM node (via the render node-map) is dispatched with [`dispatch_event`], which
// bubbles the event up the ancestor chain firing matching handlers. If a handler
// mutates the DOM, the arena's dirty flag (set by `dom::with_mut`) tells `on_frame`
// to re-render from the live tree.

fn el_add_event_listener(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::undefined());
    };
    let event = arg_string(args, 0, ctx).to_ascii_lowercase();
    let Some(handler) = args.get(1).cloned() else {
        return Ok(JsValue::undefined());
    };
    if event.is_empty() || !handler.is_callable() {
        return Ok(JsValue::undefined());
    }
    // The DOM ignores a second identical (type, handler) registration on a node.
    let dup = LISTENERS.with(|l| {
        l.borrow()
            .iter()
            .any(|e| e.node == id && e.event == event && e.handler.strict_equals(&handler))
    });
    if !dup {
        LISTENERS.with(|l| {
            l.borrow_mut().push(Listener {
                node: id,
                event,
                handler,
            })
        });
    }
    Ok(JsValue::undefined())
}

fn el_remove_event_listener(
    this: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::undefined());
    };
    let event = arg_string(args, 0, ctx).to_ascii_lowercase();
    let Some(handler) = args.get(1).cloned() else {
        return Ok(JsValue::undefined());
    };
    LISTENERS.with(|l| {
        l.borrow_mut()
            .retain(|e| !(e.node == id && e.event == event && e.handler.strict_equals(&handler)));
    });
    Ok(JsValue::undefined())
}

/// Fire a bubbling `event_type` event at arena node `target`: call every matching
/// listener on the target and each ancestor (target → root), stopping early if a
/// handler calls `stopPropagation`. Each handler runs with `this` = its element and
/// a minimal Event argument. Returns whether a handler called `preventDefault` (so
/// the caller can suppress the default action, e.g. following a link). A no-op
/// (returns `false`) when no realm exists or no listener is registered for the type.
pub(crate) fn dispatch_event(target: NodeId, event_type: &str) -> bool {
    let any = LISTENERS.with(|l| l.borrow().iter().any(|e| e.event == event_type));
    if !any {
        return false;
    }
    // Bubble path: the target then each ancestor up to the document root.
    let path = crate::dom::with(|d| {
        let mut p = Vec::new();
        let mut cur = Some(target);
        while let Some(id) = cur {
            p.push(id);
            cur = d.node(id).parent;
        }
        p
    });

    EVT_STOP.with(|c| c.set(false));
    EVT_PREVENTED.with(|c| c.set(false));

    JS.with(|cell| {
        let mut guard = cell.borrow_mut();
        let Some(ctx) = guard.as_mut() else {
            return;
        };
        for node in path {
            // Snapshot this node's handlers before calling any (a handler may mutate
            // the registry; cloning the GC handles avoids holding the borrow across
            // the call and keeps each rooted for the duration).
            let handlers: Vec<JsValue> = LISTENERS.with(|l| {
                l.borrow()
                    .iter()
                    .filter(|e| e.node == node && e.event == event_type)
                    .map(|e| e.handler.clone())
                    .collect()
            });
            if handlers.is_empty() {
                continue;
            }
            let this = element_object(ctx, node);
            let event = event_object(ctx, event_type, target, node);
            for h in handlers {
                if let Some(f) = h.as_object() {
                    if let Err(e) = f.call(&this, &[event.clone()], ctx) {
                        interstice_sdk::host_calls::log(&format!("js: event handler error: {e}"));
                    }
                }
            }
            if EVT_STOP.with(|c| c.get()) {
                break;
            }
        }
        // Let any microtasks a handler queued run before we return to re-render.
        if let Err(e) = ctx.run_jobs() {
            interstice_sdk::host_calls::log(&format!("js: microtask error: {e}"));
        }
    });

    EVT_PREVENTED.with(|c| c.get())
}

/// Build the Event object passed to a handler: `type`, `target`/`currentTarget`
/// element objects, `bubbles`, and `preventDefault`/`stopPropagation` methods that
/// set the per-dispatch flags read back by [`dispatch_event`].
fn event_object(ctx: &mut Context, event_type: &str, target: NodeId, current: NodeId) -> JsValue {
    let target_el = element_object(ctx, target);
    let current_el = element_object(ctx, current);
    ObjectInitializer::new(ctx)
        .property(js_string!("type"), js_string!(event_type.to_string()), ro())
        .property(js_string!("target"), target_el, ro())
        .property(js_string!("currentTarget"), current_el, ro())
        .property(js_string!("bubbles"), true, ro())
        .function(
            NativeFunction::from_fn_ptr(evt_prevent_default),
            js_string!("preventDefault"),
            0,
        )
        .function(
            NativeFunction::from_fn_ptr(evt_stop_propagation),
            js_string!("stopPropagation"),
            0,
        )
        .build()
        .into()
}

fn evt_prevent_default(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    EVT_PREVENTED.with(|c| c.set(true));
    Ok(JsValue::undefined())
}

fn evt_stop_propagation(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    EVT_STOP.with(|c| c.set(true));
    Ok(JsValue::undefined())
}

// ── Window, timers & the frame loop (Phase 6) ────────────────────────────────
//
// Scripts expect a global `window` (and its `self` alias) that *is* the global
// object, plus `setTimeout`/`setInterval`/`requestAnimationFrame` to defer work.
// We have no wall clock — only `FrameTick.frame`, a monotonic counter — so a delay
// in ms is quantised to whole frames (~16ms each) and callbacks fire from
// [`pump_frame`], called once per frame by `on_frame`. That same pump also drains
// the microtask queue so Promises settle. A per-call-frame loop-iteration cap
// (set in [`reset`]) keeps a runaway script from hanging the frame reducer.

/// Register `window` and `self` as globals aliasing the global object itself (so
/// `window.foo`, `window.document`, `self.setTimeout`, etc. all resolve to globals),
/// matching how a real browser's global scope is its `Window`.
fn install_window(ctx: &mut Context) {
    let global = ctx.global_object();
    for name in [js_string!("window"), js_string!("self")] {
        let _ = ctx.register_global_property(
            name,
            global.clone(),
            Attribute::WRITABLE | Attribute::CONFIGURABLE,
        );
    }
}

/// Register the timer/animation-frame globals. Each is an ordinary global function
/// (non-enumerable, writable, configurable — like a real host builtin).
fn install_timers(ctx: &mut Context) {
    let fns: [(_, usize, NativeFunctionPointer); 6] = [
        (js_string!("setTimeout"), 2, win_set_timeout as _),
        (js_string!("setInterval"), 2, win_set_interval as _),
        (js_string!("clearTimeout"), 1, win_clear_timer as _),
        (js_string!("clearInterval"), 1, win_clear_timer as _),
        (
            js_string!("requestAnimationFrame"),
            1,
            win_request_animation_frame as _,
        ),
        (
            js_string!("cancelAnimationFrame"),
            1,
            win_cancel_animation_frame as _,
        ),
    ];
    for (name, len, ptr) in fns {
        let _ = ctx.register_global_builtin_callable(name, len, NativeFunction::from_fn_ptr(ptr));
    }
}

/// Shared body of `setTimeout` (`repeating = false`) and `setInterval` (`true`):
/// record the callback with its due frame and any extra arguments, returning the
/// numeric timer id. A non-callable first argument is ignored (returns id 0).
fn schedule_timer(args: &[JsValue], ctx: &mut Context, repeating: bool) -> JsResult<JsValue> {
    let Some(callback) = args.first().cloned() else {
        return Ok(JsValue::from(0u32));
    };
    if !callback.is_callable() {
        return Ok(JsValue::from(0u32));
    }
    let ms = args.get(1).and_then(|v| v.to_number(ctx).ok()).unwrap_or(0.0);
    let frames = ms_to_frames(ms);
    let extra: Vec<JsValue> = args.iter().skip(2).cloned().collect();
    let id = NEXT_TIMER_ID.with(|n| {
        let v = n.get();
        n.set(v.wrapping_add(1).max(1));
        v
    });
    let due = FRAME.with(|f| f.get()) + frames;
    TIMERS.with(|t| {
        t.borrow_mut().push(Timer {
            id,
            callback,
            args: extra,
            due_frame: due,
            interval_frames: repeating.then_some(frames),
        })
    });
    Ok(JsValue::from(id))
}

fn win_set_timeout(_t: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    schedule_timer(args, ctx, false)
}

fn win_set_interval(_t: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    schedule_timer(args, ctx, true)
}

/// `clearTimeout`/`clearInterval` — both cancel a pending timer by id (they share a
/// single id space here, as browsers effectively do).
fn win_clear_timer(_t: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    if let Some(id) = args.first().and_then(|v| v.to_u32(ctx).ok()) {
        TIMERS.with(|t| t.borrow_mut().retain(|x| x.id != id));
    }
    Ok(JsValue::undefined())
}

fn win_request_animation_frame(
    _t: &JsValue,
    args: &[JsValue],
    _ctx: &mut Context,
) -> JsResult<JsValue> {
    let Some(callback) = args.first().cloned() else {
        return Ok(JsValue::from(0u32));
    };
    if !callback.is_callable() {
        return Ok(JsValue::from(0u32));
    }
    let id = NEXT_RAF_ID.with(|n| {
        let v = n.get();
        n.set(v.wrapping_add(1).max(1));
        v
    });
    RAF_CBS.with(|r| r.borrow_mut().push((id, callback)));
    Ok(JsValue::from(id))
}

fn win_cancel_animation_frame(
    _t: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    if let Some(id) = args.first().and_then(|v| v.to_u32(ctx).ok()) {
        RAF_CBS.with(|r| r.borrow_mut().retain(|(i, _)| *i != id));
    }
    Ok(JsValue::undefined())
}

/// Advance the JS event loop by one frame `frame` (the current `FrameTick.frame`):
/// fire every due timer (re-arming intervals, dropping one-shots), then all
/// `requestAnimationFrame` callbacks queued so far, then drain microtasks. Called
/// once per frame from `on_frame`; a no-op with nothing pending. After this returns,
/// `on_frame` re-renders if the DOM was mutated (via `dom::take_dirty`).
pub(crate) fn pump_frame(frame: u64) {
    FRAME.with(|f| f.set(frame));

    // Collect due timers under a single borrow: clone the callback+args to call
    // outside the borrow, re-arm intervals in place, and drop fired one-shots.
    let due: Vec<(JsValue, Vec<JsValue>)> = TIMERS.with(|t| {
        let mut timers = t.borrow_mut();
        let mut fired = Vec::new();
        let mut i = 0;
        while i < timers.len() {
            if timers[i].due_frame <= frame {
                fired.push((timers[i].callback.clone(), timers[i].args.clone()));
                if let Some(period) = timers[i].interval_frames {
                    timers[i].due_frame = frame + period;
                    i += 1;
                } else {
                    timers.remove(i);
                }
            } else {
                i += 1;
            }
        }
        fired
    });
    // rAF callbacks fire exactly once; drain everything queued up to now.
    let raf: Vec<JsValue> = RAF_CBS.with(|r| {
        std::mem::take(&mut *r.borrow_mut())
            .into_iter()
            .map(|(_, cb)| cb)
            .collect()
    });

    if due.is_empty() && raf.is_empty() {
        return;
    }

    JS.with(|cell| {
        let mut guard = cell.borrow_mut();
        let Some(ctx) = guard.as_mut() else {
            return;
        };
        let undefined = JsValue::undefined();
        for (cb, args) in due {
            if let Some(f) = cb.as_object() {
                if let Err(e) = f.call(&undefined, &args, ctx) {
                    interstice_sdk::host_calls::log(&format!("js: timer error: {e}"));
                }
            }
        }
        // rAF callbacks receive a timestamp; approximate ms from the frame counter.
        let timestamp = JsValue::from(frame as f64 * 16.0);
        for cb in raf {
            if let Some(f) = cb.as_object() {
                if let Err(e) = f.call(&undefined, &[timestamp.clone()], ctx) {
                    interstice_sdk::host_calls::log(&format!("js: raf error: {e}"));
                }
            }
        }
        if let Err(e) = ctx.run_jobs() {
            interstice_sdk::host_calls::log(&format!("js: microtask error: {e}"));
        }
    });
}

// ── location & history (Phase 7a) ────────────────────────────────────────────
//
// `location` and `history` are globals scripts use to inspect and change the URL.
// Reads are served from a thread_local snapshot ([`LOCATION`]/[`HIST_LEN`]) that the
// page pipeline refreshes each navigation via [`set_location`]. Writes (assigning
// `location.href`, `location.assign/replace/reload`, `history.back/forward/go`,
// `history.pushState/replaceState`) can't touch `NavState` or call the broker from a
// JS native fn (no `ctx`, and calling the broker mid-`on_http` is re-entrant), so
// they record a [`NavAction`] that `on_frame` drains and applies — the same
// queue-then-drain pattern as image/CSS fetches.

/// Snapshot of the current document's URL, in the pieces `location` exposes.
#[derive(Default, Clone)]
struct LocationData {
    href: String,
    protocol: String,
    host: String,
    pathname: String,
    search: String,
    hash: String,
    origin: String,
}

/// A navigation the page's JS requested, applied by `on_frame`. URLs are raw (as the
/// script gave them) and resolved against the live page base at apply time.
pub(crate) enum NavAction {
    /// `location.href = …` / `location.assign(…)` — navigate, pushing history.
    Load(String),
    /// `location.replace(…)` — navigate, overwriting the current history entry.
    ReplaceLoad(String),
    /// `location.reload()` — re-fetch the current entry.
    Reload,
    /// `history.back/forward/go(n)` — move by a signed delta in the history stack.
    Go(i32),
    /// `history.pushState(…, url)` — change the URL without fetching, pushing history.
    SoftPush(String),
    /// `history.replaceState(…, url)` — change the URL without fetching, in place.
    SoftReplace(String),
}

thread_local! {
    /// The current page's URL pieces, read by the `location` accessors. Refreshed by
    /// [`set_location`] on each navigation (and after a soft history update).
    static LOCATION: RefCell<LocationData> = RefCell::new(LocationData::default());
    /// `history.length` — the size of the back/forward stack.
    static HIST_LEN: Cell<u32> = const { Cell::new(0) };
    /// Navigations requested by JS this turn, drained by `on_frame`. Cleared by [`reset`].
    static PENDING_NAV: RefCell<Vec<NavAction>> = const { RefCell::new(Vec::new()) };
}

/// Refresh the JS view of the current location and history length. Called by the page
/// pipeline after adopting a page's final URL (and by `on_frame` after a soft history
/// change), so `location.href` etc. stay in sync with `NavState`.
pub(crate) fn set_location(url: &str, host: &str, path: &str, tls: bool, hist_len: u32) {
    HIST_LEN.with(|h| h.set(hist_len));
    let protocol = if tls { "https:" } else { "http:" };
    let (pathname, search, hash) = split_path(path);
    LOCATION.with(|l| {
        *l.borrow_mut() = LocationData {
            href: url.to_string(),
            protocol: protocol.to_string(),
            host: host.to_string(),
            pathname,
            search,
            hash,
            origin: format!("{protocol}//{host}"),
        };
    });
}

/// Split a path into `(pathname, search, hash)` — `/a/b?x=1#f` → (`/a/b`, `?x=1`, `#f`).
fn split_path(path: &str) -> (String, String, String) {
    let (before_hash, hash) = match path.find('#') {
        Some(i) => (&path[..i], path[i..].to_string()),
        None => (path, String::new()),
    };
    let (pathname, search) = match before_hash.find('?') {
        Some(i) => (before_hash[..i].to_string(), before_hash[i..].to_string()),
        None => (before_hash.to_string(), String::new()),
    };
    (pathname, search, hash)
}

/// Take and clear the navigations JS requested since the last drain.
pub(crate) fn take_nav_actions() -> Vec<NavAction> {
    PENDING_NAV.with(|p| std::mem::take(&mut *p.borrow_mut()))
}

fn push_nav(action: NavAction) {
    PENDING_NAV.with(|p| p.borrow_mut().push(action));
}

/// Register the `location` global (`window.location` works too, as `window` is the
/// global object). Reads come from [`LOCATION`]; the mutating members enqueue a
/// [`NavAction`].
fn install_location(ctx: &mut Context) {
    let get_href = js_fn(ctx, loc_get_href);
    let set_href = js_fn(ctx, loc_set_href);
    let get_protocol = js_fn(ctx, loc_get_protocol);
    let get_host = js_fn(ctx, loc_get_host);
    let get_pathname = js_fn(ctx, loc_get_pathname);
    let get_search = js_fn(ctx, loc_get_search);
    let get_hash = js_fn(ctx, loc_get_hash);
    let get_origin = js_fn(ctx, loc_get_origin);
    let location = ObjectInitializer::new(ctx)
        .accessor(js_string!("href"), Some(get_href), Some(set_href), ro())
        .accessor(js_string!("protocol"), Some(get_protocol), None, ro())
        .accessor(js_string!("host"), Some(get_host.clone()), None, ro())
        .accessor(js_string!("hostname"), Some(get_host), None, ro())
        .accessor(js_string!("pathname"), Some(get_pathname), None, ro())
        .accessor(js_string!("search"), Some(get_search), None, ro())
        .accessor(js_string!("hash"), Some(get_hash), None, ro())
        .accessor(js_string!("origin"), Some(get_origin), None, ro())
        .function(NativeFunction::from_fn_ptr(loc_assign), js_string!("assign"), 1)
        .function(
            NativeFunction::from_fn_ptr(loc_replace),
            js_string!("replace"),
            1,
        )
        .function(NativeFunction::from_fn_ptr(loc_reload), js_string!("reload"), 0)
        .function(
            NativeFunction::from_fn_ptr(loc_to_string),
            js_string!("toString"),
            0,
        )
        .build();
    let _ = ctx.register_global_property(
        js_string!("location"),
        location,
        Attribute::WRITABLE | Attribute::CONFIGURABLE,
    );
}

fn loc_field(pick: impl Fn(&LocationData) -> String) -> JsValue {
    js_string!(LOCATION.with(|l| pick(&l.borrow()))).into()
}

fn loc_get_href(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    Ok(loc_field(|l| l.href.clone()))
}
fn loc_get_protocol(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    Ok(loc_field(|l| l.protocol.clone()))
}
fn loc_get_host(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    Ok(loc_field(|l| l.host.clone()))
}
fn loc_get_pathname(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    Ok(loc_field(|l| l.pathname.clone()))
}
fn loc_get_search(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    Ok(loc_field(|l| l.search.clone()))
}
fn loc_get_hash(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    Ok(loc_field(|l| l.hash.clone()))
}
fn loc_get_origin(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    Ok(loc_field(|l| l.origin.clone()))
}
fn loc_to_string(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    Ok(loc_field(|l| l.href.clone()))
}

fn loc_set_href(_t: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let url = arg_string(args, 0, ctx);
    if !url.is_empty() {
        push_nav(NavAction::Load(url));
    }
    Ok(JsValue::undefined())
}
fn loc_assign(_t: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    loc_set_href(_t, args, ctx)
}
fn loc_replace(_t: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let url = arg_string(args, 0, ctx);
    if !url.is_empty() {
        push_nav(NavAction::ReplaceLoad(url));
    }
    Ok(JsValue::undefined())
}
fn loc_reload(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    push_nav(NavAction::Reload);
    Ok(JsValue::undefined())
}

/// Register the `history` global. `length` reads [`HIST_LEN`]; `state` is `null` (we
/// don't retain pushState state objects); movement/mutation members enqueue actions.
fn install_history(ctx: &mut Context) {
    let get_length = js_fn(ctx, hist_get_length);
    let get_state = js_fn(ctx, hist_get_state);
    let history = ObjectInitializer::new(ctx)
        .accessor(js_string!("length"), Some(get_length), None, ro())
        .accessor(js_string!("state"), Some(get_state), None, ro())
        .function(NativeFunction::from_fn_ptr(hist_back), js_string!("back"), 0)
        .function(
            NativeFunction::from_fn_ptr(hist_forward),
            js_string!("forward"),
            0,
        )
        .function(NativeFunction::from_fn_ptr(hist_go), js_string!("go"), 1)
        .function(
            NativeFunction::from_fn_ptr(hist_push_state),
            js_string!("pushState"),
            3,
        )
        .function(
            NativeFunction::from_fn_ptr(hist_replace_state),
            js_string!("replaceState"),
            3,
        )
        .build();
    let _ = ctx.register_global_property(
        js_string!("history"),
        history,
        Attribute::WRITABLE | Attribute::CONFIGURABLE,
    );
}

fn hist_get_length(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(HIST_LEN.with(|h| h.get())))
}
fn hist_get_state(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::null())
}
fn hist_back(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    push_nav(NavAction::Go(-1));
    Ok(JsValue::undefined())
}
fn hist_forward(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    push_nav(NavAction::Go(1));
    Ok(JsValue::undefined())
}
fn hist_go(_t: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = args.first().and_then(|v| v.to_number(ctx).ok()).unwrap_or(0.0);
    push_nav(NavAction::Go(n as i32));
    Ok(JsValue::undefined())
}
fn hist_push_state(_t: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    // pushState(state, title, url): only the url (arg 2) affects us.
    let url = arg_string(args, 2, ctx);
    if !url.is_empty() {
        push_nav(NavAction::SoftPush(url));
    }
    Ok(JsValue::undefined())
}
fn hist_replace_state(_t: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let url = arg_string(args, 2, ctx);
    if !url.is_empty() {
        push_nav(NavAction::SoftReplace(url));
    }
    Ok(JsValue::undefined())
}

// ── fetch & XMLHttpRequest (Phase 7b) ────────────────────────────────────────
//
// `fetch(url)` and `XMLHttpRequest` bridge onto the host's async HTTP path (the
// same `http_get` + `network.httpresponse.insert` round-trip the page loader
// uses). A native fn has no `ctx` to the tables and mustn't call the broker
// mid-`on_http` (re-entrant), so — like image/CSS fetches — a request is only
// *queued* here: it records a [`FetchReq`] in [`FETCH_QUEUE`] (drained by
// `on_frame`, which issues the real `http_get`) and stashes what to notify on
// completion (a promise's resolvers, or the XHR object) in [`PENDING_FETCH`],
// keyed by request id. When the response arrives, `on_http` calls
// [`deliver_fetch`], which settles the promise / fires the XHR callbacks.
//
// Fetch request ids are allocated from a high base ([`FETCH_ID_BASE`]) so they
// never collide with the low, incrementing ids the navigation/CSS/image paths
// use (`NavState.next_req_id`), letting `on_http` route by id alone.

/// The starting request id for `fetch`/XHR — far above any `NavState.next_req_id`
/// (which counts up from a handful), so the two id spaces never overlap.
const FETCH_ID_BASE: u64 = 1 << 48;

/// A queued `fetch`/XHR request awaiting its `http_get`, drained by `on_frame`.
struct FetchReq {
    req_id: u64,
    host: String,
    path: String,
    tls: bool,
}

/// What to notify when a queued fetch completes.
enum FetchTarget {
    /// A `fetch()` promise — resolve with a `Response`, or reject on transport error.
    Promise { resolve: JsFunction, reject: JsFunction },
    /// An `XMLHttpRequest` object — populate its fields and fire its callbacks.
    Xhr(JsValue),
}

/// An in-flight `fetch`/XHR, correlated to its response by `req_id`. The held
/// `JsFunction`s / `JsValue` are owned GC handles, so they stay rooted between the
/// script that started the request and the frame its response is delivered.
struct PendingFetch {
    req_id: u64,
    target: FetchTarget,
}

thread_local! {
    /// Requests queued by `fetch`/XHR this turn, drained by `on_frame`. Cleared by [`reset`].
    static FETCH_QUEUE: RefCell<Vec<FetchReq>> = const { RefCell::new(Vec::new()) };
    /// In-flight `fetch`/XHR requests keyed by `req_id`. Cleared by [`reset`].
    static PENDING_FETCH: RefCell<Vec<PendingFetch>> = const { RefCell::new(Vec::new()) };
    /// Next `fetch`/XHR request id. Reset to [`FETCH_ID_BASE`] by [`reset`].
    static NEXT_FETCH_ID: Cell<u64> = const { Cell::new(FETCH_ID_BASE) };
}

/// Allocate the next `fetch`/XHR request id (monotonic within a page).
fn alloc_fetch_id() -> u64 {
    NEXT_FETCH_ID.with(|n| {
        let v = n.get();
        n.set(v + 1);
        v
    })
}

/// Resolve a `fetch`/XHR URL argument against the current page base (from the
/// [`LOCATION`] snapshot) into `(host, path, tls)`, or `None` if unparseable.
fn resolve_fetch_url(raw: &str) -> Option<(String, String, bool)> {
    let (host, path, tls) = LOCATION.with(|l| {
        let l = l.borrow();
        (
            l.host.clone(),
            format!("{}{}", l.pathname, l.search),
            l.protocol == "https:",
        )
    });
    let loc = crate::url::resolve(&host, &path, tls, raw)?;
    Some((loc.host, loc.path, loc.tls))
}

/// Take and clear the requests `fetch`/XHR queued since the last drain — the tuples
/// `on_frame` feeds straight to `http_get`.
pub(crate) fn take_fetch_queue() -> Vec<(u64, String, String, bool)> {
    FETCH_QUEUE.with(|q| {
        std::mem::take(&mut *q.borrow_mut())
            .into_iter()
            .map(|r| (r.req_id, r.host, r.path, r.tls))
            .collect()
    })
}

/// Deliver an HTTP response to the `fetch`/XHR that asked for it. Returns `false`
/// (so `on_http` falls through to its normal routing) when `req_id` isn't a pending
/// fetch. On a hit: settle the promise (resolve with a `Response`, reject on
/// transport error) or populate the XHR and fire its callbacks, then drain the
/// microtask queue so any `.then`/handler continuations run.
pub(crate) fn deliver_fetch(req_id: u64, body: &[u8], ok: bool, error: &str) -> bool {
    let Some(pending) = PENDING_FETCH.with(|p| {
        let mut v = p.borrow_mut();
        v.iter()
            .position(|f| f.req_id == req_id)
            .map(|i| v.remove(i))
    }) else {
        return false;
    };

    let body_text = String::from_utf8_lossy(body).into_owned();
    JS.with(|cell| {
        let mut guard = cell.borrow_mut();
        let Some(ctx) = guard.as_mut() else {
            return;
        };
        match pending.target {
            FetchTarget::Promise { resolve, reject } => {
                if ok {
                    let response = build_response(ctx, &body_text, ok);
                    if let Err(e) = resolve.call(&JsValue::undefined(), &[response], ctx) {
                        interstice_sdk::host_calls::log(&format!("js: fetch resolve error: {e}"));
                    }
                } else {
                    let err = JsNativeError::typ()
                        .with_message(format!("Failed to fetch: {error}"))
                        .to_opaque(ctx);
                    if let Err(e) = reject.call(&JsValue::undefined(), &[err.into()], ctx) {
                        interstice_sdk::host_calls::log(&format!("js: fetch reject error: {e}"));
                    }
                }
            }
            FetchTarget::Xhr(obj) => {
                if let Some(o) = obj.as_object() {
                    finish_xhr(ctx, &o, &body_text, ok);
                }
            }
        }
        if let Err(e) = ctx.run_jobs() {
            interstice_sdk::host_calls::log(&format!("js: fetch microtask error: {e}"));
        }
    });
    true
}

// ── fetch ────────────────────────────────────────────────────────────────────

/// Register the global `fetch` function.
fn install_fetch(ctx: &mut Context) {
    let _ = ctx.register_global_builtin_callable(
        js_string!("fetch"),
        1,
        NativeFunction::from_fn_ptr(win_fetch),
    );
}

/// `fetch(url)` → a `Promise<Response>`. Queues the request and stashes the promise's
/// resolvers; the promise settles once `on_http` delivers the response. A bad URL
/// rejects synchronously.
fn win_fetch(_t: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let (promise, resolvers) = JsPromise::new_pending(ctx);
    let raw = arg_string(args, 0, ctx);
    match resolve_fetch_url(&raw) {
        Some((host, path, tls)) => {
            let req_id = alloc_fetch_id();
            FETCH_QUEUE.with(|q| {
                q.borrow_mut().push(FetchReq {
                    req_id,
                    host,
                    path,
                    tls,
                })
            });
            PENDING_FETCH.with(|p| {
                p.borrow_mut().push(PendingFetch {
                    req_id,
                    target: FetchTarget::Promise {
                        resolve: resolvers.resolve,
                        reject: resolvers.reject,
                    },
                })
            });
        }
        None => {
            let err = JsNativeError::typ()
                .with_message(format!("Failed to parse URL: {raw}"))
                .to_opaque(ctx);
            let _ = resolvers.reject.call(&JsValue::undefined(), &[err.into()], ctx);
        }
    }
    Ok(promise.into())
}

/// Build a minimal `Response` object over `body`: `ok`/`status`/`statusText`
/// properties plus `text()` and `json()` methods (each returning a resolved
/// promise). The body is stashed on a hidden `__body` property the methods read.
fn build_response(ctx: &mut Context, body: &str, ok: bool) -> JsValue {
    let obj = ObjectInitializer::new(ctx)
        .property(js_string!("__body"), js_string!(body), Attribute::empty())
        .property(js_string!("ok"), ok, ro())
        .property(js_string!("status"), if ok { 200u32 } else { 0u32 }, ro())
        .property(
            js_string!("statusText"),
            js_string!(if ok { "OK" } else { "" }),
            ro(),
        )
        .function(
            NativeFunction::from_fn_ptr(response_text),
            js_string!("text"),
            0,
        )
        .function(
            NativeFunction::from_fn_ptr(response_json),
            js_string!("json"),
            0,
        )
        .build();
    obj.into()
}

/// The response body as a `JsValue` string, read from the hidden `__body` property.
fn response_body(this: &JsValue, ctx: &mut Context) -> JsValue {
    this.as_object()
        .and_then(|o| o.get(js_string!("__body"), ctx).ok())
        .unwrap_or_default()
}

/// `Response.text()` → `Promise<string>` resolved with the body.
fn response_text(this: &JsValue, _a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let body = response_body(this, ctx);
    Ok(JsPromise::resolve(body, ctx).into())
}

/// `Response.json()` → `Promise` resolved with `JSON.parse(body)` (rejected if the
/// body isn't valid JSON).
fn response_json(this: &JsValue, _a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let body = response_body(this, ctx);
    match json_parse(ctx, &body) {
        Ok(v) => Ok(JsPromise::resolve(v, ctx).into()),
        Err(e) => Ok(JsPromise::reject(e, ctx).into()),
    }
}

/// Parse `body` as JSON by delegating to the realm's `JSON.parse` (so we don't pull
/// in a second parser or the `serde_json` bridge).
fn json_parse(ctx: &mut Context, body: &JsValue) -> JsResult<JsValue> {
    let global = ctx.global_object();
    let json = global.get(js_string!("JSON"), ctx)?;
    let parse = json
        .as_object()
        .ok_or_else(|| JsError::from_opaque(js_string!("JSON is not an object").into()))?
        .get(js_string!("parse"), ctx)?;
    let parse_fn = parse
        .as_object()
        .ok_or_else(|| JsError::from_opaque(js_string!("JSON.parse is not callable").into()))?;
    parse_fn.call(&JsValue::undefined(), &[body.clone()], ctx)
}

// ── XMLHttpRequest ───────────────────────────────────────────────────────────

/// Read/write + enumerable — the mutable public fields of an XHR object.
fn rw() -> Attribute {
    Attribute::WRITABLE | Attribute::ENUMERABLE | Attribute::CONFIGURABLE
}

/// Register `XMLHttpRequest` as a global constructor (so `new XMLHttpRequest()`
/// works). The constructor body returns a fresh XHR object.
fn install_xhr(ctx: &mut Context) {
    let _ = ctx.register_global_callable(
        js_string!("XMLHttpRequest"),
        0,
        NativeFunction::from_fn_ptr(xhr_construct),
    );
}

/// `new XMLHttpRequest()` — build the object with its public fields, event-handler
/// slots, and methods. `open`/`send` drive the same queue as `fetch`.
fn xhr_construct(_t: &JsValue, _a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let obj = ObjectInitializer::new(ctx)
        .property(js_string!("readyState"), 0u32, rw())
        .property(js_string!("status"), 0u32, rw())
        .property(js_string!("statusText"), js_string!(""), rw())
        .property(js_string!("responseText"), js_string!(""), rw())
        .property(js_string!("response"), js_string!(""), rw())
        .property(js_string!("onreadystatechange"), JsValue::null(), rw())
        .property(js_string!("onload"), JsValue::null(), rw())
        .property(js_string!("onerror"), JsValue::null(), rw())
        // Writable but hidden (non-enumerable): `open` re-`set`s these, so unlike the
        // Response `__body` they can't use `Attribute::empty` (which is read-only).
        .property(
            js_string!("__method"),
            js_string!(""),
            Attribute::WRITABLE | Attribute::CONFIGURABLE,
        )
        .property(
            js_string!("__url"),
            js_string!(""),
            Attribute::WRITABLE | Attribute::CONFIGURABLE,
        )
        .function(NativeFunction::from_fn_ptr(xhr_open), js_string!("open"), 2)
        .function(NativeFunction::from_fn_ptr(xhr_send), js_string!("send"), 0)
        .function(
            NativeFunction::from_fn_ptr(xhr_no_op),
            js_string!("setRequestHeader"),
            2,
        )
        .function(
            NativeFunction::from_fn_ptr(xhr_no_op),
            js_string!("abort"),
            0,
        )
        .function(
            NativeFunction::from_fn_ptr(xhr_get_all_response_headers),
            js_string!("getAllResponseHeaders"),
            0,
        )
        .build();
    Ok(obj.into())
}

/// `xhr.open(method, url)` — record the target and move to the OPENED ready state.
fn xhr_open(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    if let Some(o) = this.as_object() {
        let method = arg_string(args, 0, ctx);
        let url = arg_string(args, 1, ctx);
        let _ = o.set(js_string!("__method"), js_string!(method), false, ctx);
        let _ = o.set(js_string!("__url"), js_string!(url), false, ctx);
        let _ = o.set(js_string!("readyState"), 1u32, false, ctx);
    }
    Ok(JsValue::undefined())
}

/// `xhr.send()` — queue the request (correlated to this object) or, if the URL is
/// bad, fail it synchronously.
fn xhr_send(this: &JsValue, _a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(o) = this.as_object() else {
        return Ok(JsValue::undefined());
    };
    let url = o
        .get(js_string!("__url"), ctx)?
        .to_string(ctx)?
        .to_std_string_escaped();
    match resolve_fetch_url(&url) {
        Some((host, path, tls)) => {
            let req_id = alloc_fetch_id();
            FETCH_QUEUE.with(|q| {
                q.borrow_mut().push(FetchReq {
                    req_id,
                    host,
                    path,
                    tls,
                })
            });
            PENDING_FETCH.with(|p| {
                p.borrow_mut().push(PendingFetch {
                    req_id,
                    target: FetchTarget::Xhr(this.clone()),
                })
            });
        }
        None => finish_xhr(ctx, &o, "", false),
    }
    Ok(JsValue::undefined())
}

fn xhr_no_op(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::undefined())
}

fn xhr_get_all_response_headers(
    _t: &JsValue,
    _a: &[JsValue],
    _c: &mut Context,
) -> JsResult<JsValue> {
    Ok(js_string!("").into())
}

/// Populate a completed XHR object (ready state DONE, status, body) and fire its
/// `onreadystatechange` then `onload`/`onerror` handlers.
fn finish_xhr(ctx: &mut Context, obj: &JsObject, body: &str, ok: bool) {
    let _ = obj.set(js_string!("readyState"), 4u32, false, ctx);
    let _ = obj.set(js_string!("status"), if ok { 200u32 } else { 0u32 }, false, ctx);
    let _ = obj.set(
        js_string!("statusText"),
        js_string!(if ok { "OK" } else { "" }),
        false,
        ctx,
    );
    let _ = obj.set(js_string!("responseText"), js_string!(body), false, ctx);
    let _ = obj.set(js_string!("response"), js_string!(body), false, ctx);

    let this = JsValue::from(obj.clone());
    fire_xhr_handler(ctx, obj, "onreadystatechange", &this);
    fire_xhr_handler(ctx, obj, if ok { "onload" } else { "onerror" }, &this);
}

/// Call one XHR event-handler slot (`onload`, …) if it holds a callable.
fn fire_xhr_handler(ctx: &mut Context, obj: &JsObject, name: &str, this: &JsValue) {
    let Ok(handler) = obj.get(js_string!(name), ctx) else {
        return;
    };
    if let Some(f) = handler.as_object() {
        if f.is_callable() {
            if let Err(e) = f.call(this, &[], ctx) {
                interstice_sdk::host_calls::log(&format!("js: xhr {name} error: {e}"));
            }
        }
    }
}

// Small shared helpers for the id/class accessors.

fn attr_of(this: &JsValue, ctx: &mut Context, name: &str) -> String {
    match this_node(this, ctx) {
        Some(id) => crate::dom::with(|d| d.node(id).attr(name).unwrap_or_default().to_string()),
        None => String::new(),
    }
}

fn set_attr_from(this: &JsValue, args: &[JsValue], ctx: &mut Context, name: &str) {
    if let Some(id) = this_node(this, ctx) {
        let value = arg_string(args, 0, ctx);
        crate::dom::with_mut(|d| d.set_attr(id, name, &value));
    }
}

// ── classList ────────────────────────────────────────────────────────────────

/// `element.classList` — a fresh token-list object over the element's `class`
/// attribute (carrying the same hidden `__node`), with add/remove/toggle/contains.
fn el_get_class_list(this: &JsValue, _a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::undefined());
    };
    let obj = ObjectInitializer::new(ctx)
        .property(node_key(), id as u32, Attribute::empty())
        .function(NativeFunction::from_fn_ptr(cl_add), js_string!("add"), 1)
        .function(NativeFunction::from_fn_ptr(cl_remove), js_string!("remove"), 1)
        .function(NativeFunction::from_fn_ptr(cl_toggle), js_string!("toggle"), 1)
        .function(
            NativeFunction::from_fn_ptr(cl_contains),
            js_string!("contains"),
            1,
        )
        .build();
    Ok(obj.into())
}

fn class_tokens(id: NodeId) -> Vec<String> {
    crate::dom::with(|d| {
        d.node(id)
            .attr("class")
            .unwrap_or_default()
            .split_whitespace()
            .map(str::to_string)
            .collect()
    })
}

fn write_class_tokens(id: NodeId, tokens: &[String]) {
    crate::dom::with_mut(|d| d.set_attr(id, "class", &tokens.join(" ")));
}

fn cl_add(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::undefined());
    };
    let name = arg_string(args, 0, ctx);
    let mut tokens = class_tokens(id);
    if !name.is_empty() && !tokens.iter().any(|t| *t == name) {
        tokens.push(name);
        write_class_tokens(id, &tokens);
    }
    Ok(JsValue::undefined())
}

fn cl_remove(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::undefined());
    };
    let name = arg_string(args, 0, ctx);
    let mut tokens = class_tokens(id);
    let before = tokens.len();
    tokens.retain(|t| *t != name);
    if tokens.len() != before {
        write_class_tokens(id, &tokens);
    }
    Ok(JsValue::undefined())
}

fn cl_toggle(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::from(false));
    };
    let name = arg_string(args, 0, ctx);
    let mut tokens = class_tokens(id);
    let present = tokens.iter().any(|t| *t == name);
    if present {
        tokens.retain(|t| *t != name);
    } else if !name.is_empty() {
        tokens.push(name);
    }
    write_class_tokens(id, &tokens);
    Ok(JsValue::from(!present))
}

fn cl_contains(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::from(false));
    };
    let name = arg_string(args, 0, ctx);
    Ok(JsValue::from(class_tokens(id).iter().any(|t| *t == name)))
}

fn el_get_attribute(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::null());
    };
    let name = arg_string(args, 0, ctx).to_ascii_lowercase();
    let val = crate::dom::with(|dom| dom.node(id).attr(&name).map(str::to_string));
    Ok(match val {
        Some(v) => js_string!(v).into(),
        None => JsValue::null(),
    })
}

fn el_has_attribute(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::from(false));
    };
    let name = arg_string(args, 0, ctx).to_ascii_lowercase();
    let has = crate::dom::with(|dom| dom.node(id).attr(&name).is_some());
    Ok(JsValue::from(has))
}

/// `element.querySelector` — first descendant of `this` matching the selector.
fn el_query_selector(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::null());
    };
    let sel = arg_string(args, 0, ctx);
    query_first(ctx, Some(id), &sel)
}

fn el_query_selector_all(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsArray::new(ctx).into());
    };
    let sel = arg_string(args, 0, ctx);
    query_all(ctx, Some(id), &sel)
}

fn el_get_elements_by_tag_name(
    this: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsArray::new(ctx).into());
    };
    let sel = arg_string(args, 0, ctx);
    query_all(ctx, Some(id), &sel)
}

/// Register the `document` global: `getElementById`, `querySelector[All]`,
/// `getElementsByTagName`, plus eager `title` / `body` / `documentElement`. Built
/// during [`reset`], after the arena for the page is in place.
fn install_document(ctx: &mut Context) {
    let (body_id, html_id, title) = crate::dom::with(|dom| {
        let body = find_first_tag(dom, "body");
        let html = find_first_tag(dom, "html");
        let title = find_first_tag(dom, "title")
            .map(|t| dom.inner_text(t).trim().to_string())
            .unwrap_or_default();
        (body, html, title)
    });
    let body_el = body_id.map(|id| element_object(ctx, id));
    let html_el = html_id.map(|id| element_object(ctx, id));

    let mut init = ObjectInitializer::new(ctx);
    init.function(
        NativeFunction::from_fn_ptr(doc_get_element_by_id),
        js_string!("getElementById"),
        1,
    )
    .function(
        NativeFunction::from_fn_ptr(doc_query_selector),
        js_string!("querySelector"),
        1,
    )
    .function(
        NativeFunction::from_fn_ptr(doc_query_selector_all),
        js_string!("querySelectorAll"),
        1,
    )
    .function(
        NativeFunction::from_fn_ptr(doc_get_elements_by_tag_name),
        js_string!("getElementsByTagName"),
        1,
    )
    .function(
        NativeFunction::from_fn_ptr(doc_create_element),
        js_string!("createElement"),
        1,
    )
    .function(
        NativeFunction::from_fn_ptr(doc_create_text_node),
        js_string!("createTextNode"),
        1,
    )
    .property(js_string!("title"), js_string!(title), ro());
    if let Some(v) = body_el {
        init.property(js_string!("body"), v, ro());
    }
    if let Some(v) = html_el {
        init.property(js_string!("documentElement"), v, ro());
    }
    let document = init.build();
    let _ = ctx.register_global_property(
        js_string!("document"),
        document,
        Attribute::WRITABLE | Attribute::CONFIGURABLE,
    );
}

fn doc_get_element_by_id(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let want = arg_string(args, 0, ctx);
    let found = crate::dom::with(|dom| {
        let mut hit = None;
        for &r in dom.roots() {
            if let Some(id) = find_by_id(dom, r, &want) {
                hit = Some(id);
                break;
            }
        }
        hit
    });
    Ok(match found {
        Some(id) => element_object(ctx, id),
        None => JsValue::null(),
    })
}

fn doc_create_element(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let tag = arg_string(args, 0, ctx);
    if tag.trim().is_empty() {
        return Ok(JsValue::null());
    }
    let id = crate::dom::with_mut(|d| d.create_element(&tag));
    Ok(element_object(ctx, id))
}

fn doc_create_text_node(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let text = arg_string(args, 0, ctx);
    let id = crate::dom::with_mut(|d| d.create_text(&text));
    Ok(element_object(ctx, id))
}

fn doc_query_selector(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let sel = arg_string(args, 0, ctx);
    query_first(ctx, None, &sel)
}

fn doc_query_selector_all(
    _this: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let sel = arg_string(args, 0, ctx);
    query_all(ctx, None, &sel)
}

fn doc_get_elements_by_tag_name(
    _this: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let sel = arg_string(args, 0, ctx);
    query_all(ctx, None, &sel)
}

/// Run a selector query over the whole document (`scope: None`) or a subtree, and
/// return the first matching element object, or `null`.
fn query_first(ctx: &mut Context, scope: Option<NodeId>, selector: &str) -> JsResult<JsValue> {
    let Some(sel) = SimpleSelector::parse(selector) else {
        return Ok(JsValue::null());
    };
    let found = crate::dom::with(|dom| {
        let mut hit = None;
        for root in scope_roots(dom, scope) {
            find_match(dom, root, &sel, scope.is_some(), &mut |id| {
                hit = Some(id);
                true // stop at first
            });
            if hit.is_some() {
                break;
            }
        }
        hit
    });
    Ok(match found {
        Some(id) => element_object(ctx, id),
        None => JsValue::null(),
    })
}

/// Like [`query_first`] but collects every match into a JS array (in document order).
fn query_all(ctx: &mut Context, scope: Option<NodeId>, selector: &str) -> JsResult<JsValue> {
    let Some(sel) = SimpleSelector::parse(selector) else {
        return Ok(JsArray::new(ctx).into());
    };
    let ids = crate::dom::with(|dom| {
        let mut ids = Vec::new();
        for root in scope_roots(dom, scope) {
            find_match(dom, root, &sel, scope.is_some(), &mut |id| {
                ids.push(id);
                false // keep going
            });
        }
        ids
    });
    let arr = JsArray::new(ctx);
    for id in ids {
        let el = element_object(ctx, id);
        arr.push(el, ctx)?;
    }
    Ok(arr.into())
}

/// The starting node(s) for a query: a scoped element's direct children (so the
/// scope element itself is excluded, matching `Element.querySelector`), or every
/// document root.
fn scope_roots(dom: &Dom, scope: Option<NodeId>) -> Vec<NodeId> {
    match scope {
        Some(id) => dom.node(id).children.clone(),
        None => dom.roots().to_vec(),
    }
}

/// Depth-first (document order) walk from `id`, invoking `hit(node)` on each match.
/// `hit` returns `true` to stop the whole walk early. `skip_self` is unused here
/// because [`scope_roots`] already excludes the scope element; roots are always
/// eligible.
fn find_match(
    dom: &Dom,
    id: NodeId,
    sel: &SimpleSelector,
    _skip_self: bool,
    hit: &mut impl FnMut(NodeId) -> bool,
) -> bool {
    if sel.matches(dom, id) && hit(id) {
        return true;
    }
    for &c in &dom.node(id).children {
        if find_match(dom, c, sel, false, hit) {
            return true;
        }
    }
    false
}

/// Find the first element (document order) whose `id` attribute equals `want`.
fn find_by_id(dom: &Dom, id: NodeId, want: &str) -> Option<NodeId> {
    if dom.node(id).attr("id") == Some(want) {
        return Some(id);
    }
    for &c in &dom.node(id).children {
        if let Some(found) = find_by_id(dom, c, want) {
            return Some(found);
        }
    }
    None
}

/// The first element with tag `tag` anywhere in document order.
fn find_first_tag(dom: &Dom, tag: &str) -> Option<NodeId> {
    fn walk(dom: &Dom, id: NodeId, tag: &str) -> Option<NodeId> {
        if dom.node(id).tag() == Some(tag) {
            return Some(id);
        }
        for &c in &dom.node(id).children {
            if let Some(found) = walk(dom, c, tag) {
                return Some(found);
            }
        }
        None
    }
    dom.roots().iter().find_map(|&r| walk(dom, r, tag))
}

/// A single compound selector: an optional type, an optional `#id`, and any number
/// of `.class`es (e.g. `div#main.active`). Combinators and grouping aren't modelled
/// — a descendant selector like `nav a` is reduced to its subject (`a`), matching
/// the whole document, which over-matches but never under-matches (the same
/// pragmatic simplification the CSS cascade in `css.rs` makes for combinators).
struct SimpleSelector {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
}

impl SimpleSelector {
    fn parse(input: &str) -> Option<SimpleSelector> {
        // Take the first selector of a group, then its subject (last compound).
        let first = input.split(',').next().unwrap_or("").trim();
        let subject = first.split_whitespace().last().unwrap_or("").trim();
        if subject.is_empty() {
            return None;
        }
        let mut sel = SimpleSelector {
            tag: None,
            id: None,
            classes: Vec::new(),
        };
        // Scan into runs delimited by `#` / `.`; the leading run (no sigil) is the type.
        let mut kind = b'\0'; // '\0' = type, '#' = id, '.' = class
        let mut cur = String::new();
        let flush = |kind: u8, cur: &mut String, sel: &mut SimpleSelector| {
            if cur.is_empty() {
                return;
            }
            let val = std::mem::take(cur);
            match kind {
                b'#' => sel.id = Some(val),
                b'.' => sel.classes.push(val),
                _ => {
                    if val != "*" {
                        sel.tag = Some(val.to_ascii_lowercase());
                    }
                }
            }
        };
        for ch in subject.chars() {
            if ch == '#' || ch == '.' {
                flush(kind, &mut cur, &mut sel);
                kind = ch as u8;
            } else {
                cur.push(ch);
            }
        }
        flush(kind, &mut cur, &mut sel);
        if sel.tag.is_none() && sel.id.is_none() && sel.classes.is_empty() {
            // A bare `*` matches everything.
            return Some(sel);
        }
        Some(sel)
    }

    fn matches(&self, dom: &Dom, id: NodeId) -> bool {
        let node = dom.node(id);
        let Some(tag) = node.tag() else {
            return false; // text/comment nodes never match
        };
        if let Some(t) = &self.tag {
            if !t.eq_ignore_ascii_case(tag) {
                return false;
            }
        }
        if let Some(want) = &self.id {
            if node.attr("id") != Some(want.as_str()) {
                return false;
            }
        }
        if !self.classes.is_empty() {
            let classes = node.attr("class").unwrap_or_default();
            for want in &self.classes {
                if !classes.split_whitespace().any(|c| c == want) {
                    return false;
                }
            }
        }
        true
    }
}
