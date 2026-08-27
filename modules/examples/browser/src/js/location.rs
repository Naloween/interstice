use super::*;

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
pub(crate) struct LocationData {
    pub(crate) href: String,
    pub(crate) protocol: String,
    pub(crate) host: String,
    pub(crate) pathname: String,
    pub(crate) search: String,
    pub(crate) hash: String,
    pub(crate) origin: String,
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
    pub(crate) static LOCATION: RefCell<LocationData> = RefCell::new(LocationData::default());
    /// `history.length` — the size of the back/forward stack.
    static HIST_LEN: Cell<u32> = const { Cell::new(0) };
    /// Navigations requested by JS this turn, drained by `on_frame`. Cleared by [`reset`].
    pub(crate) static PENDING_NAV: RefCell<Vec<NavAction>> = const { RefCell::new(Vec::new()) };
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
pub(crate) fn split_path(path: &str) -> (String, String, String) {
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

pub(crate) fn push_nav(action: NavAction) {
    PENDING_NAV.with(|p| p.borrow_mut().push(action));
}

/// Register the `location` global (`window.location` works too, as `window` is the
/// global object). Reads come from [`LOCATION`]; the mutating members enqueue a
/// [`NavAction`].
pub(crate) fn install_location(ctx: &mut Context) {
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

pub(crate) fn loc_field(pick: impl Fn(&LocationData) -> String) -> JsValue {
    js_string!(LOCATION.with(|l| pick(&l.borrow()))).into()
}

pub(crate) fn loc_get_href(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    Ok(loc_field(|l| l.href.clone()))
}
pub(crate) fn loc_get_protocol(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    Ok(loc_field(|l| l.protocol.clone()))
}
pub(crate) fn loc_get_host(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    Ok(loc_field(|l| l.host.clone()))
}
pub(crate) fn loc_get_pathname(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    Ok(loc_field(|l| l.pathname.clone()))
}
pub(crate) fn loc_get_search(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    Ok(loc_field(|l| l.search.clone()))
}
pub(crate) fn loc_get_hash(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    Ok(loc_field(|l| l.hash.clone()))
}
pub(crate) fn loc_get_origin(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    Ok(loc_field(|l| l.origin.clone()))
}
pub(crate) fn loc_to_string(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    Ok(loc_field(|l| l.href.clone()))
}

pub(crate) fn loc_set_href(_t: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let url = arg_string(args, 0, ctx);
    if !url.is_empty() {
        push_nav(NavAction::Load(url));
    }
    Ok(JsValue::undefined())
}
pub(crate) fn loc_assign(_t: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    loc_set_href(_t, args, ctx)
}
pub(crate) fn loc_replace(_t: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let url = arg_string(args, 0, ctx);
    if !url.is_empty() {
        push_nav(NavAction::ReplaceLoad(url));
    }
    Ok(JsValue::undefined())
}
pub(crate) fn loc_reload(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    push_nav(NavAction::Reload);
    Ok(JsValue::undefined())
}

/// Register the `history` global. `length` reads [`HIST_LEN`]; `state` is `null` (we
/// don't retain pushState state objects); movement/mutation members enqueue actions.
pub(crate) fn install_history(ctx: &mut Context) {
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

pub(crate) fn hist_get_length(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::from(HIST_LEN.with(|h| h.get())))
}
pub(crate) fn hist_get_state(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::null())
}
pub(crate) fn hist_back(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    push_nav(NavAction::Go(-1));
    Ok(JsValue::undefined())
}
pub(crate) fn hist_forward(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    push_nav(NavAction::Go(1));
    Ok(JsValue::undefined())
}
pub(crate) fn hist_go(_t: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = args.first().and_then(|v| v.to_number(ctx).ok()).unwrap_or(0.0);
    push_nav(NavAction::Go(n as i32));
    Ok(JsValue::undefined())
}
pub(crate) fn hist_push_state(_t: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    // pushState(state, title, url): only the url (arg 2) affects us.
    let url = arg_string(args, 2, ctx);
    if !url.is_empty() {
        push_nav(NavAction::SoftPush(url));
    }
    Ok(JsValue::undefined())
}
pub(crate) fn hist_replace_state(_t: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let url = arg_string(args, 2, ctx);
    if !url.is_empty() {
        push_nav(NavAction::SoftReplace(url));
    }
    Ok(JsValue::undefined())
}

