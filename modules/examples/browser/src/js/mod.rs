//! Page JavaScript, on [Boa](https://boajs.dev) (the pure-Rust engine that
//! builds for `wasm32-unknown-unknown`) over Blitz's DOM.
//!
//! The split:
//!
//! - This file is a thin *host* layer: a `__host` object of primitive
//!   functions over node ids (tree walking, attributes, mutation, selectors,
//!   layout queries, focus, scrolling, URL parsing, a deterministic clock and
//!   RNG, fetch ids, storage).
//! - [`prelude.js`](prelude.js) builds the web platform on top of it in JS:
//!   `Node`/`Element`/`Document` wrappers, events with capture/bubble,
//!   timers, `fetch`/`XMLHttpRequest`, `location`/`history`, storage, `URL`…
//!   and the script loader.
//!
//! **Determinism.** A realm lives in a document's [`DocCache`](crate::cache::DocCache),
//! which is derived data: another instance rebuilds it by re-running the
//! page's scripts and replaying the document's `DocEvent` log. So JS only
//! ever sees inputs that are in that log or in tables: user input, timer
//! ticks (`EV_TICK`, carrying the virtual clock), fetch completions
//! (`EV_FETCH`, whose body is in the `Resource` table). Time is virtual
//! (`performance.now()` advances only by ticks), `Math.random` is seeded
//! per document, and side effects (navigation, storage writes) are only
//! honoured when an event is applied live, never during a replay.

use crate::providers;
use blitz_dom::{BaseDocument, LocalName, NodeId, Namespace, NodeData, QualName, ScrollBehavior, ScrollLogicalPosition, ns};
use blitz_traits::navigation::NavigationOptions;
use boa_engine::context::time::{Clock, JsInstant};
use boa_engine::object::ObjectInitializer;
use boa_engine::object::builtins::JsArray;
use boa_engine::property::Attribute as PropAttr;
use boa_engine::{Context, JsError, JsNativeError, JsResult, JsString, JsValue, NativeFunction, Source, js_string};
use std::cell::Cell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

const PRELUDE: &str = include_str!("prelude.js");

/// Script-visible state that is not in the DOM.
pub struct HostState {
    /// Wall-clock epoch of the document (ms), from `Document.t0`.
    pub t0: f64,
    /// Virtual time since `t0` (ms); advanced by `EV_TICK` only.
    pub clock: f64,
    rng: u64,
    next_fetch: u32,
    /// Outstanding fetches: id → absolute URL.
    pub fetches: BTreeMap<u32, String>,
    /// Event types some listener was registered for.
    pub listened: HashSet<String>,
    /// Applied live (side effects allowed) rather than replayed.
    pub live: bool,
    /// `localStorage` contents for this origin, and writes made live
    /// (name, `None` = removed); the engine stores them in `WebStorage`.
    storage: HashMap<String, String>,
    pub storage_writes: Vec<(String, Option<String>)>,
    /// Layout may be stale (DOM mutated since the last resolve).
    layout_dirty: bool,
}

pub struct Js {
    ctx: Context,
    host: Box<HostState>,
}

thread_local! {
    static DOC: Cell<*mut BaseDocument> = const { Cell::new(std::ptr::null_mut()) };
    static HOST: Cell<*mut HostState> = const { Cell::new(std::ptr::null_mut()) };
    /// (t0, clock) of the realm currently entered, for Boa's clock.
    static CLOCK: Cell<(f64, f64)> = const { Cell::new((0.0, 0.0)) };
    /// Raw id of the entered document's root (script id 0).
    static ROOT: Cell<u64> = const { Cell::new(0) };
}

/// Node ids as script numbers: the raw (versioned) id, except the document
/// itself, which scripts know as 0. Raw ids stay well below 2^53.
fn to_js(id: NodeId) -> f64 {
    let raw = id.as_u64();
    if raw == ROOT.with(|r| r.get()) { 0.0 } else { raw as f64 }
}

fn from_js(n: f64) -> NodeId {
    if n == 0.0 { NodeId::from_u64(ROOT.with(|r| r.get())) } else { NodeId::from_u64(n as u64) }
}

/// Boa's clock: the entered realm's virtual time.
struct VirtualClock;

impl Clock for VirtualClock {
    fn now(&self) -> JsInstant {
        let ms = CLOCK.with(|c| c.get().1).max(0.0) as u64;
        JsInstant::new(ms / 1000, ((ms % 1000) * 1_000_000) as u32)
    }
    fn system_time_millis(&self) -> i64 {
        let (t0, clock) = CLOCK.with(|c| c.get());
        (t0 + clock) as i64
    }
}

/// Does this document have anything for a JS realm to run?
pub fn has_scripts(doc: &BaseDocument) -> bool {
    doc.query_selector("script").ok().flatten().is_some()
        || doc.query_selector("[onclick],[onload],[oninput],[onchange],[onsubmit],[onkeydown]").ok().flatten().is_some()
}

impl Js {
    /// Create the realm and install the platform; scripts don't run until
    /// [`Js::boot`].
    pub fn new(doc_id: u64, t0: f64, origin_storage: Vec<(String, String)>) -> Js {
        let mut ctx = Context::builder().clock(Rc::new(VirtualClock)).build().expect("boa context");
        // A runaway loop throws instead of hanging the reducer.
        ctx.runtime_limits_mut().set_loop_iteration_limit(20_000_000);
        ctx.runtime_limits_mut().set_recursion_limit(2048);
        install_host(&mut ctx);
        let host = Box::new(HostState {
            t0,
            clock: 0.0,
            rng: doc_id.wrapping_mul(0x9E3779B97F4A7C15) ^ 0xD1B54A32D192ED03,
            next_fetch: 1,
            fetches: BTreeMap::new(),
            listened: HashSet::new(),
            live: false,
            storage: origin_storage.into_iter().collect(),
            storage_writes: Vec::new(),
            layout_dirty: true,
        });
        Js { ctx, host }
    }

    pub fn host(&mut self) -> &mut HostState {
        &mut self.host
    }

    /// Run `f` with this realm bound to `doc`, then drain the job queue.
    pub fn enter<R>(&mut self, doc: &mut BaseDocument, f: impl FnOnce(&mut Context) -> R) -> R {
        let prev = (DOC.with(|d| d.get()), HOST.with(|h| h.get()), CLOCK.with(|c| c.get()), ROOT.with(|r| r.get()));
        ROOT.with(|r| r.set(doc.root_node().id.as_u64()));
        DOC.with(|d| d.set(doc as *mut _));
        HOST.with(|h| h.set(&mut *self.host as *mut _));
        CLOCK.with(|c| c.set((self.host.t0, self.host.clock)));
        let r = f(&mut self.ctx);
        if let Err(e) = self.ctx.run_jobs() {
            crate::log(&format!("js: {}", e));
        }
        DOC.with(|d| d.set(prev.0));
        HOST.with(|h| h.set(prev.1));
        CLOCK.with(|c| c.set(prev.2));
        ROOT.with(|r| r.set(prev.3));
        r
    }

    /// Call a global function defined by the prelude.
    fn call(&mut self, doc: &mut BaseDocument, name: &str, args: &[JsValue]) -> JsValue {
        self.enter(doc, |ctx| {
            let f = ctx.global_object().get(JsString::from(name), ctx).ok().and_then(|f| f.as_object());
            match f {
                Some(f) => f.call(&JsValue::undefined(), args, ctx).unwrap_or_else(|e| {
                    crate::log(&format!("js: {name}: {e}"));
                    JsValue::undefined()
                }),
                None => JsValue::undefined(),
            }
        })
    }

    /// Run the page's scripts (and the dynamic ones they load, as their
    /// fetches complete).
    pub fn boot(&mut self, doc: &mut BaseDocument) {
        // The prelude wraps the document's existing nodes, so it runs entered.
        let ok = self.enter(doc, |ctx| ctx.eval(Source::from_bytes(PRELUDE)).map_err(|e| crate::log(&format!("js: prelude failed: {e}"))).is_ok());
        if ok {
            self.call(doc, "__boot", &[]);
        }
    }

    /// Evaluate `src` in the page realm and stringify the result. Test hook
    /// (`browser_script`'s `js` command): not logged, so it must not mutate.
    pub fn eval(&mut self, doc: &mut BaseDocument, src: &str) -> String {
        self.enter(doc, |ctx| match ctx.eval(Source::from_bytes(src)) {
            Ok(v) => v.to_string(ctx).map(|s| s.to_std_string_escaped()).unwrap_or_else(|e| format!("<{e}>")),
            Err(e) => format!("<{e}>"),
        })
    }

    /// Advance the virtual clock and run due timers / animation frames.
    pub fn tick(&mut self, doc: &mut BaseDocument, clock: f64) {
        self.host.clock = self.host.clock.max(clock);
        let t = JsValue::from(self.host.clock);
        self.call(doc, "__tick", &[t]);
    }

    /// Virtual time at which the realm next wants a tick, if any.
    pub fn next_due(&mut self, doc: &mut BaseDocument) -> Option<f64> {
        let v = self.call(doc, "__next", &[]);
        v.as_number().filter(|n| *n >= 0.0)
    }

    /// A fetch completed (`body` = `None`: network error).
    pub fn fetch_done(&mut self, doc: &mut BaseDocument, id: u32, body: Option<&[u8]>) {
        let Some(url) = self.host.fetches.remove(&id) else { return };
        let (status, text) = match body {
            Some(b) => (200.0, crate::encoding::decode_body(b.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(b))),
            None => (0.0, String::new()),
        };
        let args = [JsValue::from(id), JsValue::from(status), JsValue::from(JsString::from(text.as_str())), JsValue::from(JsString::from(url.as_str()))];
        self.call(doc, "__fetchDone", &args);
    }

    /// Dispatch a DOM event to script listeners along `chain` (target
    /// first). Returns (default prevented, propagation stopped).
    pub fn dispatch(&mut self, doc: &mut BaseDocument, name: &str, chain: &[NodeId], bubbles: bool, cancelable: bool, init: &str) -> (bool, bool) {
        let r = self.enter(doc, |ctx| {
            let ids: Vec<JsValue> = chain.iter().map(|&i| JsValue::from(to_js(i))).collect();
            let arr: JsValue = JsArray::from_iter(ids, ctx).into();
            let f = ctx.global_object().get(js_string!("__dispatch"), ctx).ok().and_then(|f| f.as_object())?;
            let args = [JsValue::from(JsString::from(name)), arr, JsValue::from(bubbles), JsValue::from(cancelable), JsValue::from(JsString::from(init))];
            match f.call(&JsValue::undefined(), &args, ctx) {
                Ok(v) => v.as_number(),
                Err(e) => {
                    crate::log(&format!("js: dispatch {name}: {e}"));
                    None
                }
            }
        });
        let flags = r.unwrap_or(0.0) as u32;
        (flags & 1 != 0, flags & 2 != 0)
    }

    /// Would a `name` event at `chain` reach any script handler?
    pub fn wants(&self, doc: &BaseDocument, name: &str, chain: &[NodeId]) -> bool {
        if self.host.listened.contains(name) {
            return true;
        }
        let attr = format!("on{name}");
        chain.iter().any(|&id| doc.get_node(id).and_then(|n| n.attr(LocalName::from(attr.as_str()))).is_some())
    }

    /// Does script react to pointer motion (so moves must be logged)?
    pub fn wants_motion(&self) -> bool {
        const MOTION: [&str; 10] = [
            "mousemove", "pointermove", "mouseover", "mouseout", "mouseenter", "mouseleave", "pointerover", "pointerout",
            "pointerenter", "pointerleave",
        ];
        MOTION.iter().any(|t| self.host.listened.contains(*t))
    }
}

// ── Host functions ───────────────────────────────────────────────────────────

fn doc<'a>() -> JsResult<&'a mut BaseDocument> {
    let p = DOC.with(|d| d.get());
    if p.is_null() {
        return Err(JsNativeError::error().with_message("no document").into());
    }
    // SAFETY: set by `Js::enter` from a live `&mut BaseDocument` for the
    // duration of the call; host functions never hold it across calls.
    Ok(unsafe { &mut *p })
}

fn host<'a>() -> JsResult<&'a mut HostState> {
    let p = HOST.with(|h| h.get());
    if p.is_null() {
        return Err(JsNativeError::error().with_message("no realm").into());
    }
    // SAFETY: as for `doc`.
    Ok(unsafe { &mut *p })
}

fn arg(args: &[JsValue], i: usize) -> JsValue {
    args.get(i).cloned().unwrap_or_default()
}

fn num(args: &[JsValue], i: usize) -> f64 {
    arg(args, i).as_number().unwrap_or(f64::NAN)
}

fn string(args: &[JsValue], i: usize, ctx: &mut Context) -> JsResult<String> {
    let v = arg(args, i);
    if v.is_undefined() || v.is_null() {
        return Ok(String::new());
    }
    Ok(v.to_string(ctx)?.to_std_string_escaped())
}

/// A node id argument that names an existing node.
fn node_arg(args: &[JsValue], i: usize) -> JsResult<NodeId> {
    let n = num(args, i);
    let d = doc()?;
    if n.is_finite() && n >= 0.0 && d.get_node(from_js(n)).is_some() {
        Ok(from_js(n))
    } else {
        Err(JsNativeError::typ().with_message("invalid node").into())
    }
}

fn s(v: &str) -> JsValue {
    JsValue::from(JsString::from(v))
}

fn id_or_neg(id: Option<NodeId>) -> JsValue {
    JsValue::from(id.map(to_js).unwrap_or(-1.0))
}

fn array(vals: impl IntoIterator<Item = JsValue>, ctx: &mut Context) -> JsValue {
    JsArray::from_iter(vals, ctx).into()
}

fn ids(v: impl IntoIterator<Item = NodeId>, ctx: &mut Context) -> JsValue {
    array(v.into_iter().map(|i| JsValue::from(to_js(i))), ctx)
}

fn syntax(e: impl std::fmt::Debug) -> JsError {
    JsNativeError::syntax().with_message(format!("invalid selector: {e:?}")).into()
}

fn attr_name(name: &str) -> QualName {
    QualName::new(None, ns!(), LocalName::from(name))
}

fn mutated() -> JsResult<()> {
    host()?.layout_dirty = true;
    providers::REDRAW.store(true, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

/// Bring layout up to date before a geometry query.
fn layout() -> JsResult<&'static mut BaseDocument> {
    let d = doc()?;
    let h = host()?;
    if h.layout_dirty {
        h.layout_dirty = false;
        d.resolve((h.t0 + h.clock) / 1000.0);
    }
    Ok(d)
}

type HostFn = fn(&JsValue, &[JsValue], &mut Context) -> JsResult<JsValue>;

fn install_host(ctx: &mut Context) {
    let fns: &[(&str, HostFn, usize)] = &[
        ("log", h_log, 2),
        ("kind", h_kind, 1),
        ("tag", h_tag, 1),
        ("ns", h_ns, 1),
        ("parent", h_parent, 1),
        ("children", h_children, 1),
        ("text", h_text, 1),
        ("setText", h_set_text, 2),
        ("getAttr", h_get_attr, 2),
        ("setAttr", h_set_attr, 3),
        ("removeAttr", h_remove_attr, 2),
        ("attrNames", h_attr_names, 1),
        ("innerHTML", h_inner_html, 1),
        ("outerHTML", h_outer_html, 1),
        ("setInnerHTML", h_set_inner_html, 2),
        ("createElement", h_create_element, 2),
        ("createText", h_create_text, 1),
        ("createComment", h_create_comment, 1),
        ("append", h_append, 2),
        ("insertBefore", h_insert_before, 3),
        ("remove", h_remove, 1),
        ("clone", h_clone, 2),
        ("query", h_query, 2),
        ("queryAll", h_query_all, 2),
        ("matches", h_matches, 2),
        ("byId", h_by_id, 1),
        ("rect", h_rect, 1),
        ("box", h_box, 1),
        ("computed", h_computed, 2),
        ("value", h_value, 1),
        ("setValue", h_set_value, 2),
        ("checked", h_checked, 1),
        ("setChecked", h_set_checked, 2),
        ("focus", h_focus, 1),
        ("blur", h_blur, 0),
        ("activeElement", h_active, 0),
        ("scrollPos", h_scroll_pos, 1),
        ("scrollTo", h_scroll_to, 3),
        ("scrollIntoView", h_scroll_into_view, 2),
        ("viewport", h_viewport, 0),
        ("hit", h_hit, 2),
        ("url", h_url, 0),
        ("navigate", h_navigate, 1),
        ("now", h_now, 0),
        ("timeOrigin", h_time_origin, 0),
        ("random", h_random, 0),
        ("fetch", h_fetch, 1),
        ("listen", h_listen, 1),
        ("storageGet", h_storage_get, 1),
        ("storageSet", h_storage_set, 2),
        ("storageKeys", h_storage_keys, 0),
        ("urlParse", h_url_parse, 2),
        ("live", h_live, 0),
    ];
    let mut init = ObjectInitializer::new(ctx);
    for (name, f, arity) in fns {
        init.function(NativeFunction::from_fn_ptr(*f), JsString::from(*name), *arity);
    }
    let obj = init.build();
    let _ = ctx.register_global_property(js_string!("__host"), obj, PropAttr::all());
}

fn h_log(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let level = string(args, 0, ctx)?;
    let msg = string(args, 1, ctx)?;
    // Replays (another instance rebuilding the page) would repeat the log.
    if host()?.live {
        crate::log(&format!("[js {level}] {msg}"));
    }
    Ok(JsValue::undefined())
}

fn h_kind(_: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let n = num(args, 0);
    let k = doc()?.get_node(from_js(n)).filter(|_| n >= 0.0).map(|n| match &n.data {
        NodeData::Document(_) => 9,
        NodeData::Element(_) | NodeData::AnonymousBlock(_) => 1,
        NodeData::Text(_) => 3,
        NodeData::Comment { .. } => 8,
    });
    Ok(JsValue::from(k.unwrap_or(0)))
}

fn h_tag(_: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = node_arg(args, 0)?;
    Ok(s(doc()?.get_node(id).and_then(|n| n.element_data()).map(|e| &*e.name.local).unwrap_or("")))
}

fn h_ns(_: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = node_arg(args, 0)?;
    Ok(s(doc()?.get_node(id).and_then(|n| n.element_data()).map(|e| &*e.name.ns).unwrap_or("")))
}

fn h_parent(_: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = node_arg(args, 0)?;
    Ok(id_or_neg(doc()?.get_node(id).and_then(|n| n.parent)))
}

fn h_children(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = node_arg(args, 0)?;
    let c = doc()?.get_node(id).map(|n| n.children.clone()).unwrap_or_default();
    Ok(ids(c, ctx))
}

fn h_text(_: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = node_arg(args, 0)?;
    let n = doc()?.get_node(id).unwrap();
    Ok(s(&match &n.data {
        NodeData::Comment { contents } => contents.clone(),
        _ => n.text_content(),
    }))
}

fn h_set_text(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = node_arg(args, 0)?;
    let text = string(args, 1, ctx)?;
    let d = doc()?;
    match &d.get_node(id).unwrap().data {
        NodeData::Text(_) => d.mutate().set_node_text(id, &text),
        NodeData::Comment { .. } => {
            if let Some(NodeData::Comment { contents }) = d.get_node_mut(id).map(|n| &mut n.data) {
                *contents = text;
            }
        }
        NodeData::Element(_) | NodeData::Document(_) => {
            let mut m = d.mutate();
            detach_children(&mut m, id);
            if !text.is_empty() {
                let t = m.create_text_node(&text);
                m.append_children(id, &[t]);
            }
        }
        _ => {}
    }
    mutated()?;
    Ok(JsValue::undefined())
}

fn h_get_attr(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = node_arg(args, 0)?;
    let name = string(args, 1, ctx)?.to_ascii_lowercase();
    Ok(doc()?.get_node(id).unwrap().attr(LocalName::from(name.as_str())).map(s).unwrap_or(JsValue::null()))
}

fn h_set_attr(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = node_arg(args, 0)?;
    let name = string(args, 1, ctx)?.to_ascii_lowercase();
    let value = string(args, 2, ctx)?;
    let d = doc()?;
    if d.get_node(id).unwrap().is_element() && !name.is_empty() {
        d.mutate().set_attribute(id, attr_name(&name), &value);
        mutated()?;
    }
    Ok(JsValue::undefined())
}

fn h_remove_attr(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = node_arg(args, 0)?;
    let name = string(args, 1, ctx)?.to_ascii_lowercase();
    let d = doc()?;
    if d.get_node(id).unwrap().attr(LocalName::from(name.as_str())).is_some() {
        d.mutate().clear_attribute(id, attr_name(&name));
        mutated()?;
    }
    Ok(JsValue::undefined())
}

fn h_attr_names(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = node_arg(args, 0)?;
    let names: Vec<JsValue> =
        doc()?.get_node(id).unwrap().attrs().unwrap_or(&[]).iter().map(|a| s(&a.name.local)).collect();
    Ok(array(names, ctx))
}

fn h_inner_html(_: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = node_arg(args, 0)?;
    let d = doc()?;
    let n = d.get_node(id).unwrap();
    let mut out = String::new();
    for &c in &n.children {
        if let Some(c) = d.get_node(c) {
            out.push_str(&c.outer_html());
        }
    }
    Ok(s(&out))
}

fn h_outer_html(_: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = node_arg(args, 0)?;
    Ok(s(&doc()?.get_node(id).unwrap().outer_html()))
}

/// Detach (not drop) every child: in the DOM removed nodes stay alive and
/// scripts routinely keep using them (jQuery's `buildFragment` collects
/// `tmp.childNodes`, then clears `tmp`). Freed slots would turn those ids into
/// "invalid node" errors. Detached nodes live until the document is dropped.
fn detach_children(m: &mut blitz_dom::DocumentMutator, id: NodeId) {
    for c in m.child_ids(id) {
        m.remove_node(c);
    }
}

fn h_set_inner_html(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = node_arg(args, 0)?;
    let html = string(args, 1, ctx)?;
    let d = doc()?;
    if d.get_node(id).unwrap().is_element() {
        let mut m = d.mutate();
        detach_children(&mut m, id);
        m.set_inner_html(id, &html);
        mutated()?;
    }
    Ok(JsValue::undefined())
}

fn h_create_element(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let tag = string(args, 0, ctx)?;
    let ns_uri = string(args, 1, ctx)?;
    let ns: Namespace = if ns_uri.is_empty() { ns!(html) } else { Namespace::from(ns_uri.as_str()) };
    let local = if ns == ns!(html) { tag.to_ascii_lowercase() } else { tag };
    let id = doc()?.mutate().create_element(QualName::new(None, ns, LocalName::from(local.as_str())), Vec::new());
    Ok(JsValue::from(to_js(id)))
}

fn h_create_text(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let text = string(args, 0, ctx)?;
    Ok(JsValue::from(to_js(doc()?.mutate().create_text_node(&text))))
}

fn h_create_comment(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let text = string(args, 0, ctx)?;
    Ok(JsValue::from(to_js(doc()?.mutate().create_comment_node(&text))))
}

/// Would making `child` a child of `parent` create a cycle?
fn is_ancestor(d: &BaseDocument, child: NodeId, parent: NodeId) -> bool {
    let mut cur = Some(parent);
    while let Some(id) = cur {
        if id == child {
            return true;
        }
        cur = d.get_node(id).and_then(|n| n.parent);
    }
    false
}

fn h_append(_: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let parent = node_arg(args, 0)?;
    let child = node_arg(args, 1)?;
    let d = doc()?;
    if is_ancestor(d, child, parent) {
        return Err(JsNativeError::error().with_message("HierarchyRequestError").into());
    }
    d.mutate().append_children(parent, &[child]);
    mutated()?;
    Ok(JsValue::undefined())
}

fn h_insert_before(_: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let parent = node_arg(args, 0)?;
    let child = node_arg(args, 1)?;
    let d = doc()?;
    if is_ancestor(d, child, parent) {
        return Err(JsNativeError::error().with_message("HierarchyRequestError").into());
    }
    let anchor = num(args, 2);
    let anchor = (anchor >= 0.0)
        .then_some(from_js(anchor))
        .filter(|&a| a != child && d.get_node(a).is_some_and(|n| n.parent == Some(parent)));
    match anchor {
        Some(a) => d.mutate().insert_nodes_before(a, &[child]),
        None if num(args, 2) == to_js(child) => {}
        None => d.mutate().append_children(parent, &[child]),
    }
    mutated()?;
    Ok(JsValue::undefined())
}

fn h_remove(_: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = node_arg(args, 0)?;
    let d = doc()?;
    if d.get_node(id).unwrap().parent.is_some() {
        d.mutate().remove_node(id);
        mutated()?;
    }
    Ok(JsValue::undefined())
}

fn h_clone(_: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = node_arg(args, 0)?;
    let deep = arg(args, 1).to_boolean();
    let d = doc()?;
    let n = d.get_node(id).unwrap();
    let new = if deep {
        d.mutate().deep_clone_node(id)
    } else {
        match &n.data {
            NodeData::Element(e) => {
                let (name, attrs) = (e.name.clone(), e.attrs().to_vec());
                d.mutate().create_element(name, attrs)
            }
            NodeData::Text(t) => {
                let t = t.content.clone();
                d.mutate().create_text_node(&t)
            }
            NodeData::Comment { contents } => {
                let c = contents.clone();
                d.mutate().create_comment_node(&c)
            }
            _ => d.mutate().deep_clone_node(id),
        }
    };
    Ok(JsValue::from(to_js(new)))
}

fn h_query(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let scope = node_arg(args, 0)?;
    let sel = string(args, 1, ctx)?;
    let d = doc()?;
    let r = if scope == d.root_node().id { d.query_selector(&sel) } else { d.query_selector_in(scope, &sel) };
    Ok(id_or_neg(r.map_err(syntax)?))
}

fn h_query_all(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let scope = node_arg(args, 0)?;
    let sel = string(args, 1, ctx)?;
    let d = doc()?;
    let r = if scope == d.root_node().id { d.query_selector_all(&sel) } else { d.query_selector_all_in(scope, &sel) };
    Ok(ids(r.map_err(syntax)?, ctx))
}

fn h_matches(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = node_arg(args, 0)?;
    let sel = string(args, 1, ctx)?;
    Ok(JsValue::from(doc()?.matches_selector(id, &sel).map_err(syntax)?))
}

fn h_by_id(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = string(args, 0, ctx)?;
    Ok(id_or_neg(doc()?.get_element_by_id(&id)))
}

fn h_rect(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = node_arg(args, 0)?;
    let d = layout()?;
    Ok(match d.get_client_bounding_rect(id) {
        Some(r) => array([r.x, r.y, r.width, r.height].map(JsValue::from), ctx),
        None => JsValue::null(),
    })
}

/// [client w, client h, scroll w, scroll h, scroll x, scroll y, offset x, offset y]
fn h_box(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = node_arg(args, 0)?;
    let d = layout()?;
    let n = d.get_node(id).unwrap();
    if !n.is_element() {
        return Ok(JsValue::null());
    }
    let l = n.final_layout();
    let ov = n.scrollable_overflow();
    let so = n.scroll_offset();
    let cw = (l.size.width - l.border.left - l.border.right - l.scrollbar_size.width) as f64;
    let ch = (l.size.height - l.border.top - l.border.bottom - l.scrollbar_size.height) as f64;
    let v = [
        cw.max(0.0),
        ch.max(0.0),
        ov.width().max(cw),
        ov.height().max(ch),
        so.x,
        so.y,
        l.location.x as f64,
        l.location.y as f64,
    ];
    Ok(array(v.map(JsValue::from), ctx))
}

fn h_computed(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = node_arg(args, 0)?;
    let prop = string(args, 1, ctx)?;
    let d = layout()?;
    if !d.get_node(id).unwrap().is_element() {
        return Ok(s(""));
    }
    Ok(s(&d.resolved_style_value(id, &prop)))
}

fn h_value(_: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = node_arg(args, 0)?;
    let n = doc()?.get_node(id).unwrap();
    let Some(e) = n.element_data() else { return Ok(JsValue::null()) };
    if let Some(t) = e.text_input_data() {
        return Ok(s(&t.editor.text().to_string()));
    }
    Ok(JsValue::null())
}

fn h_set_value(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = node_arg(args, 0)?;
    let v = string(args, 1, ctx)?;
    let d = doc()?;
    if d.get_node(id).unwrap().element_data().is_some_and(|e| e.text_input_data().is_some()) {
        // Blitz routes a `value` attribute write to the text editor.
        d.mutate().set_attribute(id, attr_name("value"), &v);
        mutated()?;
    }
    Ok(JsValue::undefined())
}

fn h_checked(_: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = node_arg(args, 0)?;
    let c = doc()?.get_node(id).unwrap().element_data().and_then(|e| e.checkbox_input_checked());
    Ok(c.map(JsValue::from).unwrap_or(JsValue::null()))
}

fn h_set_checked(_: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = node_arg(args, 0)?;
    let v = arg(args, 1).to_boolean();
    let d = doc()?;
    if let Some(e) = d.get_node_mut(id).and_then(|n| n.element_data_mut()) {
        if e.checkbox_input_checked().is_some() {
            e.set_checkbox_input_checked(v);
        }
    }
    d.snapshot_node(id);
    mutated()?;
    Ok(JsValue::undefined())
}

fn h_focus(_: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = node_arg(args, 0)?;
    doc()?.set_focus_to(id);
    mutated()?;
    Ok(JsValue::undefined())
}

fn h_blur(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    doc()?.clear_focus();
    mutated()?;
    Ok(JsValue::undefined())
}

fn h_active(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    Ok(id_or_neg(doc()?.get_focussed_node_id()))
}

/// Scroll offset of an element, or of the viewport for -1.
fn h_scroll_pos(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = num(args, 0);
    let d = doc()?;
    let p = if id < 0.0 {
        d.viewport_scroll()
    } else {
        let id = node_arg(args, 0)?;
        *layout()?.get_node(id).unwrap().scroll_offset()
    };
    Ok(array([JsValue::from(p.x), JsValue::from(p.y)], ctx))
}

fn h_scroll_to(_: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = num(args, 0);
    let (x, y) = (num(args, 1), num(args, 2));
    let d = layout()?;
    if id < 0.0 {
        let cur = d.viewport_scroll();
        let x = if x.is_finite() { x } else { cur.x };
        let y = if y.is_finite() { y } else { cur.y };
        d.scroll_viewport_by(x - cur.x, y - cur.y);
    } else {
        let id = node_arg(args, 0)?;
        let cur = *d.get_node(id).unwrap().scroll_offset();
        let x = if x.is_finite() { x } else { cur.x };
        let y = if y.is_finite() { y } else { cur.y };
        d.scroll_to(id, x, y, ScrollBehavior::Instant);
    }
    providers::REDRAW.store(true, std::sync::atomic::Ordering::Relaxed);
    Ok(JsValue::undefined())
}

fn h_scroll_into_view(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = node_arg(args, 0)?;
    let pos = match string(args, 1, ctx)?.as_str() {
        "center" => ScrollLogicalPosition::Center,
        "end" => ScrollLogicalPosition::End,
        "nearest" => ScrollLogicalPosition::Nearest,
        _ => ScrollLogicalPosition::Start,
    };
    layout()?.scroll_into_view(id, ScrollBehavior::Instant, pos, ScrollLogicalPosition::Nearest);
    providers::REDRAW.store(true, std::sync::atomic::Ordering::Relaxed);
    Ok(JsValue::undefined())
}

fn h_viewport(_: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let (w, h) = doc()?.viewport().window_size;
    Ok(array([JsValue::from(w), JsValue::from(h)], ctx))
}

fn h_hit(_: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let (x, y) = (num(args, 0) as f32, num(args, 1) as f32);
    Ok(id_or_neg(layout()?.element_from_point(x, y)))
}

fn h_url(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    Ok(s(doc()?.url().as_str()))
}

fn h_navigate(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let target = string(args, 0, ctx)?;
    let d = doc()?;
    let Ok(url) = d.url().join(&target) else { return Ok(JsValue::from(false)) };
    if matches!(url.scheme(), "javascript") {
        return Ok(JsValue::from(false));
    }
    providers::NAV_QUEUE.lock().unwrap().push(NavigationOptions::new(url, None, d.id()));
    Ok(JsValue::from(true))
}

fn h_now(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(host()?.clock))
}

fn h_time_origin(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(host()?.t0))
}

fn h_random(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let h = host()?;
    h.rng = h.rng.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = h.rng;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^= z >> 31;
    Ok(JsValue::from((z >> 11) as f64 / (1u64 << 53) as f64))
}

/// Start a GET; returns its id (completion arrives as `__fetchDone`), or
/// -1 for a URL we can't fetch.
fn h_fetch(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let target = string(args, 0, ctx)?;
    let Ok(url) = doc()?.url().join(&target) else { return Ok(JsValue::from(-1)) };
    if !matches!(url.scheme(), "http" | "https" | "data") {
        return Ok(JsValue::from(-1));
    }
    let mut url = url;
    url.set_fragment(None);
    let h = host()?;
    let id = h.next_fetch;
    h.next_fetch += 1;
    h.fetches.insert(id, url.to_string());
    Ok(JsValue::from(id))
}

fn h_listen(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let t = string(args, 0, ctx)?;
    host()?.listened.insert(t);
    Ok(JsValue::undefined())
}

fn h_storage_get(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let k = string(args, 0, ctx)?;
    Ok(host()?.storage.get(&k).map(|v| s(v)).unwrap_or(JsValue::null()))
}

fn h_storage_set(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let k = string(args, 0, ctx)?;
    let v = arg(args, 1);
    let v = if v.is_null() || v.is_undefined() { None } else { Some(v.to_string(ctx)?.to_std_string_escaped()) };
    let h = host()?;
    match &v {
        Some(v) => h.storage.insert(k.clone(), v.clone()),
        None => h.storage.remove(&k),
    };
    if h.live {
        h.storage_writes.push((k, v));
    }
    Ok(JsValue::undefined())
}

fn h_storage_keys(_: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let mut keys: Vec<String> = host()?.storage.keys().cloned().collect();
    keys.sort();
    Ok(array(keys.iter().map(|k| s(k)), ctx))
}

/// `[href, protocol, username, password, host, hostname, port, pathname,
/// search, hash, origin]`, or null if `href` doesn't parse.
fn h_url_parse(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let href = string(args, 0, ctx)?;
    let base = arg(args, 1);
    let parsed = if base.is_undefined() || base.is_null() {
        url::Url::parse(&href)
    } else {
        let base = base.to_string(ctx)?.to_std_string_escaped();
        url::Url::parse(&base).and_then(|b| b.join(&href))
    };
    let Ok(u) = parsed else { return Ok(JsValue::null()) };
    let host = match (u.host_str(), u.port()) {
        (Some(h), Some(p)) => format!("{h}:{p}"),
        (Some(h), None) => h.to_string(),
        _ => String::new(),
    };
    let q = u.query().map(|q| format!("?{q}")).filter(|q| q.len() > 1).unwrap_or_default();
    let f = u.fragment().map(|f| format!("#{f}")).filter(|f| f.len() > 1).unwrap_or_default();
    let parts = [
        u.as_str().to_string(),
        format!("{}:", u.scheme()),
        u.username().to_string(),
        u.password().unwrap_or("").to_string(),
        host,
        u.host_str().unwrap_or("").to_string(),
        u.port().map(|p| p.to_string()).unwrap_or_default(),
        u.path().to_string(),
        q,
        f,
        u.origin().ascii_serialization(),
    ];
    Ok(array(parts.iter().map(|p| s(p)), ctx))
}

fn h_live(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(host()?.live))
}
