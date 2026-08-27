use super::*;

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
pub(crate) const FETCH_ID_BASE: u64 = 1 << 48;

/// A queued `fetch`/XHR request awaiting its `http_get`, drained by `on_frame`.
pub(crate) struct FetchReq {
    req_id: u64,
    host: String,
    path: String,
    tls: bool,
}

/// What to notify when a queued fetch completes.
pub(crate) enum FetchTarget {
    /// A `fetch()` promise — resolve with a `Response`, or reject on transport error.
    Promise { resolve: JsFunction, reject: JsFunction },
    /// An `XMLHttpRequest` object — populate its fields and fire its callbacks.
    Xhr(JsValue),
}

/// An in-flight `fetch`/XHR, correlated to its response by `req_id`. The held
/// `JsFunction`s / `JsValue` are owned GC handles, so they stay rooted between the
/// script that started the request and the frame its response is delivered.
pub(crate) struct PendingFetch {
    req_id: u64,
    target: FetchTarget,
}

thread_local! {
    /// Requests queued by `fetch`/XHR this turn, drained by `on_frame`. Cleared by [`reset`].
    pub(crate) static FETCH_QUEUE: RefCell<Vec<FetchReq>> = const { RefCell::new(Vec::new()) };
    /// In-flight `fetch`/XHR requests keyed by `req_id`. Cleared by [`reset`].
    pub(crate) static PENDING_FETCH: RefCell<Vec<PendingFetch>> = const { RefCell::new(Vec::new()) };
    /// Next `fetch`/XHR request id. Reset to [`FETCH_ID_BASE`] by [`reset`].
    pub(crate) static NEXT_FETCH_ID: Cell<u64> = const { Cell::new(FETCH_ID_BASE) };
}

/// Allocate the next `fetch`/XHR request id (monotonic within a page).
pub(crate) fn alloc_fetch_id() -> u64 {
    NEXT_FETCH_ID.with(|n| {
        let v = n.get();
        n.set(v + 1);
        v
    })
}

/// Resolve a `fetch`/XHR URL argument against the current page base (from the
/// [`LOCATION`] snapshot) into `(host, path, tls)`, or `None` if unparseable.
pub(crate) fn resolve_fetch_url(raw: &str) -> Option<(String, String, bool)> {
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

    let body_text = crate::encoding::decode_body(body);
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
pub(crate) fn install_fetch(ctx: &mut Context) {
    let _ = ctx.register_global_builtin_callable(
        js_string!("fetch"),
        1,
        NativeFunction::from_fn_ptr(win_fetch),
    );
}

/// `fetch(url)` → a `Promise<Response>`. Queues the request and stashes the promise's
/// resolvers; the promise settles once `on_http` delivers the response. A bad URL
/// rejects synchronously.
pub(crate) fn win_fetch(_t: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
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
pub(crate) fn build_response(ctx: &mut Context, body: &str, ok: bool) -> JsValue {
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
pub(crate) fn response_body(this: &JsValue, ctx: &mut Context) -> JsValue {
    this.as_object()
        .and_then(|o| o.get(js_string!("__body"), ctx).ok())
        .unwrap_or_default()
}

/// `Response.text()` → `Promise<string>` resolved with the body.
pub(crate) fn response_text(this: &JsValue, _a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let body = response_body(this, ctx);
    Ok(JsPromise::resolve(body, ctx).into())
}

/// `Response.json()` → `Promise` resolved with `JSON.parse(body)` (rejected if the
/// body isn't valid JSON).
pub(crate) fn response_json(this: &JsValue, _a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let body = response_body(this, ctx);
    match json_parse(ctx, &body) {
        Ok(v) => Ok(JsPromise::resolve(v, ctx).into()),
        Err(e) => Ok(JsPromise::reject(e, ctx).into()),
    }
}

/// Parse `body` as JSON by delegating to the realm's `JSON.parse` (so we don't pull
/// in a second parser or the `serde_json` bridge).
pub(crate) fn json_parse(ctx: &mut Context, body: &JsValue) -> JsResult<JsValue> {
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
pub(crate) fn rw() -> Attribute {
    Attribute::WRITABLE | Attribute::ENUMERABLE | Attribute::CONFIGURABLE
}

/// Register `XMLHttpRequest` as a global constructor (so `new XMLHttpRequest()`
/// works). The constructor body returns a fresh XHR object.
pub(crate) fn install_xhr(ctx: &mut Context) {
    let _ = ctx.register_global_callable(
        js_string!("XMLHttpRequest"),
        0,
        NativeFunction::from_fn_ptr(xhr_construct),
    );
}

/// `new XMLHttpRequest()` — build the object with its public fields, event-handler
/// slots, and methods. `open`/`send` drive the same queue as `fetch`.
pub(crate) fn xhr_construct(_t: &JsValue, _a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
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
pub(crate) fn xhr_open(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
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
pub(crate) fn xhr_send(this: &JsValue, _a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
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

pub(crate) fn xhr_no_op(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::undefined())
}

pub(crate) fn xhr_get_all_response_headers(
    _t: &JsValue,
    _a: &[JsValue],
    _c: &mut Context,
) -> JsResult<JsValue> {
    Ok(js_string!("").into())
}

/// Populate a completed XHR object (ready state DONE, status, body) and fire its
/// `onreadystatechange` then `onload`/`onerror` handlers.
pub(crate) fn finish_xhr(ctx: &mut Context, obj: &JsObject, body: &str, ok: bool) {
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
pub(crate) fn fire_xhr_handler(ctx: &mut Context, obj: &JsObject, name: &str, this: &JsValue) {
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

pub(crate) fn attr_of(this: &JsValue, ctx: &mut Context, name: &str) -> String {
    match this_node(this, ctx) {
        Some(id) => crate::dom::with(|d| d.node(id).attr(name).unwrap_or_default().to_string()),
        None => String::new(),
    }
}

pub(crate) fn set_attr_from(this: &JsValue, args: &[JsValue], ctx: &mut Context, name: &str) {
    if let Some(id) = this_node(this, ctx) {
        let value = arg_string(args, 0, ctx);
        crate::dom::with_mut(|d| d.set_attr(id, name, &value));
    }
}

