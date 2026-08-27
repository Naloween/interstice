use super::*;

// ── Events (Phase 5) ─────────────────────────────────────────────────────────
//
// `addEventListener` records a (node, type, handler) row; a click resolved to a
// DOM node (via the render node-map) is dispatched with [`dispatch_event`], which
// bubbles the event up the ancestor chain firing matching handlers. If a handler
// mutates the DOM, the arena's dirty flag (set by `dom::with_mut`) tells `on_frame`
// to re-render from the live tree.

pub(crate) fn el_add_event_listener(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
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

pub(crate) fn el_remove_event_listener(
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
pub(crate) fn event_object(ctx: &mut Context, event_type: &str, target: NodeId, current: NodeId) -> JsValue {
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

pub(crate) fn evt_prevent_default(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    EVT_PREVENTED.with(|c| c.set(true));
    Ok(JsValue::undefined())
}

pub(crate) fn evt_stop_propagation(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    EVT_STOP.with(|c| c.set(true));
    Ok(JsValue::undefined())
}

