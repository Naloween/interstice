use super::*;

/// Register the `document` global: `getElementById`, `querySelector[All]`,
/// `getElementsByTagName`, plus eager `title` / `body` / `documentElement`. Built
/// during [`reset`], after the arena for the page is in place.
pub(crate) fn install_document(ctx: &mut Context) {
    let (body_id, html_id, head_id, title) = crate::dom::with(|dom| {
        let body = find_first_tag(dom, "body");
        let html = find_first_tag(dom, "html");
        let head = find_first_tag(dom, "head");
        let title = find_first_tag(dom, "title")
            .map(|t| dom.inner_text(t).trim().to_string())
            .unwrap_or_default();
        (body, html, head, title)
    });
    let body_el = body_id.map(|id| element_object(ctx, id));
    let html_el = html_id.map(|id| element_object(ctx, id));
    // Some pages `document.head.appendChild(<link rel=preconnect>)`; fall back to the
    // <html> element so the property is always an object (never `undefined`, which
    // would throw on the first `.appendChild`), matching a real always-present head.
    let head_el = head_id.or(html_id).map(|id| element_object(ctx, id));

    // The node document/window listeners bubble to. Prefer <html>; fall back to the
    // first arena root so document.addEventListener still fires on a malformed page.
    let doc_node = html_id.or_else(|| crate::dom::with(|d| d.roots().first().copied()));
    DOC_NODE.with(|c| c.set(doc_node));

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
    .function(
        NativeFunction::from_fn_ptr(doc_add_event_listener),
        js_string!("addEventListener"),
        2,
    )
    .function(
        NativeFunction::from_fn_ptr(doc_remove_event_listener),
        js_string!("removeEventListener"),
        2,
    )
    // The DOM arena is fully built before any script runs, so from a script's point
    // of view the document is already parsed: report "complete" so readiness helpers
    // (`doWhenReady`, jQuery-style `$(fn)`) run their callbacks immediately.
    .property(js_string!("readyState"), js_string!("complete"), ro())
    // No cookie jar; scripts that read `document.cookie` just see none set.
    .property(js_string!("cookie"), js_string!(""), rw())
    .property(js_string!("title"), js_string!(title), ro());
    if let Some(v) = body_el {
        init.property(js_string!("body"), v, ro());
    }
    if let Some(v) = html_el {
        init.property(js_string!("documentElement"), v, ro());
    }
    if let Some(v) = head_el {
        init.property(js_string!("head"), v, ro());
    }
    let document = init.build();
    let _ = ctx.register_global_property(
        js_string!("document"),
        document,
        Attribute::WRITABLE | Attribute::CONFIGURABLE,
    );
}

/// Register a document/window-level listener on the [`DOC_NODE`] root. A no-op if
/// the page has no root or the handler isn't callable. Shared by `document.*` and
/// `window.*` `addEventListener` — both funnel to the same root so a bubbled event
/// (click/change/…) reaches them, and non-bubbling ones (`load`) are fired at the
/// root by [`fire_document_ready`].
pub(crate) fn add_doc_listener(args: &[JsValue], _ctx: &Context) {
    let Some(node) = DOC_NODE.with(|c| c.get()) else {
        return;
    };
    let event = args
        .first()
        .and_then(|v| v.as_string())
        .map(|s| s.to_std_string_escaped().to_ascii_lowercase())
        .unwrap_or_default();
    let Some(handler) = args.get(1).cloned() else {
        return;
    };
    if event.is_empty() || !handler.is_callable() {
        return;
    }
    let dup = LISTENERS.with(|l| {
        l.borrow()
            .iter()
            .any(|e| e.node == node && e.event == event && e.handler.strict_equals(&handler))
    });
    if !dup {
        LISTENERS.with(|l| {
            l.borrow_mut().push(Listener {
                node,
                event,
                handler,
            })
        });
    }
}

pub(crate) fn remove_doc_listener(args: &[JsValue]) {
    let Some(node) = DOC_NODE.with(|c| c.get()) else {
        return;
    };
    let event = args
        .first()
        .and_then(|v| v.as_string())
        .map(|s| s.to_std_string_escaped().to_ascii_lowercase())
        .unwrap_or_default();
    let Some(handler) = args.get(1).cloned() else {
        return;
    };
    LISTENERS.with(|l| {
        l.borrow_mut()
            .retain(|e| !(e.node == node && e.event == event && e.handler.strict_equals(&handler)));
    });
}

pub(crate) fn doc_add_event_listener(
    _t: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    add_doc_listener(args, ctx);
    Ok(JsValue::undefined())
}

pub(crate) fn doc_remove_event_listener(
    _t: &JsValue,
    args: &[JsValue],
    _ctx: &mut Context,
) -> JsResult<JsValue> {
    remove_doc_listener(args);
    Ok(JsValue::undefined())
}

pub(crate) fn win_add_event_listener(
    _t: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    add_doc_listener(args, ctx);
    Ok(JsValue::undefined())
}

pub(crate) fn win_remove_event_listener(
    _t: &JsValue,
    args: &[JsValue],
    _ctx: &mut Context,
) -> JsResult<JsValue> {
    remove_doc_listener(args);
    Ok(JsValue::undefined())
}

/// Fire `DOMContentLoaded` then `load` at the document root, after the page's
/// external scripts have run and registered their handlers. `readyState` is already
/// "complete", so `doWhenReady`-style helpers ran synchronously — this covers code
/// that instead attached directly via `window.addEventListener("load", …)`.
pub(crate) fn fire_document_ready() {
    if let Some(node) = DOC_NODE.with(|c| c.get()) {
        dispatch_event(node, "domcontentloaded");
        dispatch_event(node, "load");
    }
}

/// Register the DOM interface constructors pages feature-detect and patch, chiefly
/// `Element.prototype`. Our element objects aren't real instances of these (they're
/// plain objects with live accessors), so these exist only so polyfill code like
/// `Element.prototype.matches || (Element.prototype.matches = …)` runs without a
/// `ReferenceError` — the actual `matches`/`closest` live natively on each element.
pub(crate) fn install_dom_constructors(ctx: &mut Context) {
    for name in [
        js_string!("Element"),
        js_string!("Node"),
        js_string!("HTMLElement"),
        js_string!("HTMLDocument"),
        js_string!("Event"),
    ] {
        let prototype = ObjectInitializer::new(ctx).build();
        let ctor = ObjectInitializer::new(ctx)
            .property(js_string!("prototype"), prototype, Attribute::WRITABLE)
            .build();
        let _ =
            ctx.register_global_property(name, ctor, Attribute::WRITABLE | Attribute::CONFIGURABLE);
    }
}

/// A minimal `navigator`: enough language/UA surface for portal scripts that pick a
/// suggested-language list or gate on the UA. `sendBeacon` is a no-op returning
/// false (we don't do analytics); do-not-track is unset.
pub(crate) fn install_navigator(ctx: &mut Context) {
    let langs = JsArray::new(ctx);
    let _ = langs.push(js_string!("en-US"), ctx);
    let _ = langs.push(js_string!("en"), ctx);
    let navigator = ObjectInitializer::new(ctx)
        .property(js_string!("language"), js_string!("en-US"), ro())
        .property(js_string!("languages"), langs, ro())
        .property(
            js_string!("userAgent"),
            js_string!("Mozilla/5.0 (interstice-browser)"),
            ro(),
        )
        .property(js_string!("doNotTrack"), JsValue::null(), ro())
        .function(
            NativeFunction::from_fn_ptr(|_t, _a, _c| Ok(JsValue::from(false))),
            js_string!("sendBeacon"),
            1,
        )
        .build();
    let _ = ctx.register_global_property(
        js_string!("navigator"),
        navigator,
        Attribute::WRITABLE | Attribute::CONFIGURABLE,
    );
}

/// An in-memory `localStorage`/`sessionStorage` (per page realm — not persisted).
/// Portal code wraps every access in try/catch and tolerates missing values, so a
/// volatile store is enough to keep it on its happy path without real persistence.
pub(crate) fn install_storage(ctx: &mut Context) {
    for name in [js_string!("localStorage"), js_string!("sessionStorage")] {
        let store = ObjectInitializer::new(ctx)
            .function(
                NativeFunction::from_fn_ptr(storage_get_item),
                js_string!("getItem"),
                1,
            )
            .function(
                NativeFunction::from_fn_ptr(storage_set_item),
                js_string!("setItem"),
                2,
            )
            .function(
                NativeFunction::from_fn_ptr(storage_remove_item),
                js_string!("removeItem"),
                1,
            )
            .function(
                NativeFunction::from_fn_ptr(storage_clear),
                js_string!("clear"),
                0,
            )
            .build();
        let _ = ctx.register_global_property(
            name,
            store,
            Attribute::WRITABLE | Attribute::CONFIGURABLE,
        );
    }
}

/// Register the `Image` constructor. `new Image()` is used mostly for preloading or
/// analytics pings (`new Image().src = url`); we don't actually fetch, so the object
/// just holds writable `src`/`onload`/`onerror`/`width`/`height` fields and never
/// fires its callbacks — enough that construction and assignment don't throw.
pub(crate) fn install_image(ctx: &mut Context) {
    let _ = ctx.register_global_callable(
        js_string!("Image"),
        0,
        NativeFunction::from_fn_ptr(image_construct),
    );
}

pub(crate) fn image_construct(
    _t: &JsValue,
    _args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let obj = ObjectInitializer::new(ctx)
        .property(js_string!("src"), js_string!(""), rw())
        .property(js_string!("onload"), JsValue::null(), rw())
        .property(js_string!("onerror"), JsValue::null(), rw())
        .property(js_string!("width"), 0, rw())
        .property(js_string!("height"), 0, rw())
        .property(js_string!("complete"), false, rw())
        .build();
    Ok(obj.into())
}

pub(crate) fn storage_get_item(
    _t: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let key = arg_string(args, 0, ctx);
    Ok(STORAGE.with(|s| {
        s.borrow()
            .get(&key)
            .map(|v| JsValue::from(js_string!(v.clone())))
            .unwrap_or(JsValue::null())
    }))
}

pub(crate) fn storage_set_item(
    _t: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let key = arg_string(args, 0, ctx);
    let val = arg_string(args, 1, ctx);
    STORAGE.with(|s| s.borrow_mut().insert(key, val));
    Ok(JsValue::undefined())
}

pub(crate) fn storage_remove_item(
    _t: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let key = arg_string(args, 0, ctx);
    STORAGE.with(|s| s.borrow_mut().remove(&key));
    Ok(JsValue::undefined())
}

pub(crate) fn storage_clear(
    _t: &JsValue,
    _args: &[JsValue],
    _ctx: &mut Context,
) -> JsResult<JsValue> {
    STORAGE.with(|s| s.borrow_mut().clear());
    Ok(JsValue::undefined())
}

pub(crate) fn doc_get_element_by_id(
    _this: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
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

pub(crate) fn doc_create_element(
    _this: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let tag = arg_string(args, 0, ctx);
    if tag.trim().is_empty() {
        return Ok(JsValue::null());
    }
    let id = crate::dom::with_mut(|d| d.create_element(&tag));
    Ok(element_object(ctx, id))
}

pub(crate) fn doc_create_text_node(
    _this: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let text = arg_string(args, 0, ctx);
    let id = crate::dom::with_mut(|d| d.create_text(&text));
    Ok(element_object(ctx, id))
}

pub(crate) fn doc_query_selector(
    _this: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let sel = arg_string(args, 0, ctx);
    query_first(ctx, None, &sel)
}

pub(crate) fn doc_query_selector_all(
    _this: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let sel = arg_string(args, 0, ctx);
    query_all(ctx, None, &sel)
}

pub(crate) fn doc_get_elements_by_tag_name(
    _this: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> JsResult<JsValue> {
    let sel = arg_string(args, 0, ctx);
    query_all(ctx, None, &sel)
}

/// Run a selector query over the whole document (`scope: None`) or a subtree, and
/// return the first matching element object, or `null`.
pub(crate) fn query_first(
    ctx: &mut Context,
    scope: Option<NodeId>,
    selector: &str,
) -> JsResult<JsValue> {
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
pub(crate) fn query_all(
    ctx: &mut Context,
    scope: Option<NodeId>,
    selector: &str,
) -> JsResult<JsValue> {
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

/// `element.matches(selector)` — whether the element itself matches the (simple)
/// selector. Used directly by pages and as the primitive behind [`el_closest`].
pub(crate) fn el_matches(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::from(false));
    };
    let sel_str = arg_string(args, 0, ctx);
    let Some(sel) = SimpleSelector::parse(&sel_str) else {
        return Ok(JsValue::from(false));
    };
    let hit = crate::dom::with(|dom| sel.matches(dom, id));
    Ok(JsValue::from(hit))
}

/// `element.closest(selector)` — the element itself or its nearest ancestor that
/// matches the selector, or `null`. Portal scripts use this to hit-test clicks
/// (e.g. `e.target.closest("#search-form")`).
pub(crate) fn el_closest(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::null());
    };
    let sel_str = arg_string(args, 0, ctx);
    let Some(sel) = SimpleSelector::parse(&sel_str) else {
        return Ok(JsValue::null());
    };
    let found = crate::dom::with(|dom| {
        let mut cur = Some(id);
        while let Some(n) = cur {
            if sel.matches(dom, n) {
                return Some(n);
            }
            cur = dom.node(n).parent;
        }
        None
    });
    Ok(match found {
        Some(n) => element_object(ctx, n),
        None => JsValue::null(),
    })
}
