use super::*;

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
pub(crate) fn install_window(ctx: &mut Context) {
    let global = ctx.global_object();
    for name in [js_string!("window"), js_string!("self")] {
        let _ = ctx.register_global_property(
            name,
            global.clone(),
            Attribute::WRITABLE | Attribute::CONFIGURABLE,
        );
    }
    // `window.addEventListener("load"/"click"/…, …)` — registered as globals (window
    // *is* the global object) that funnel to the document root, so a bubbled event
    // reaches them and [`fire_document_ready`] can fire non-bubbling `load`.
    let _ = ctx.register_global_callable(
        js_string!("addEventListener"),
        2,
        NativeFunction::from_fn_ptr(win_add_event_listener),
    );
    let _ = ctx.register_global_callable(
        js_string!("removeEventListener"),
        2,
        NativeFunction::from_fn_ptr(win_remove_event_listener),
    );
    // Scrolling the window is a no-op (fixed viewport), but `window.scroll(0,0)` and
    // friends are called at load; they must exist and be callable.
    for name in [
        js_string!("scroll"),
        js_string!("scrollTo"),
        js_string!("scrollBy"),
    ] {
        let _ = ctx.register_global_callable(name, 2, NativeFunction::from_fn_ptr(el_noop));
    }
}

/// Register the timer/animation-frame globals. Each is an ordinary global function
/// (non-enumerable, writable, configurable — like a real host builtin).
pub(crate) fn install_timers(ctx: &mut Context) {
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
pub(crate) fn schedule_timer(args: &[JsValue], ctx: &mut Context, repeating: bool) -> JsResult<JsValue> {
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

pub(crate) fn win_set_timeout(_t: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    schedule_timer(args, ctx, false)
}

pub(crate) fn win_set_interval(_t: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    schedule_timer(args, ctx, true)
}

/// `clearTimeout`/`clearInterval` — both cancel a pending timer by id (they share a
/// single id space here, as browsers effectively do).
pub(crate) fn win_clear_timer(_t: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    if let Some(id) = args.first().and_then(|v| v.to_u32(ctx).ok()) {
        TIMERS.with(|t| t.borrow_mut().retain(|x| x.id != id));
    }
    Ok(JsValue::undefined())
}

pub(crate) fn win_request_animation_frame(
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

pub(crate) fn win_cancel_animation_frame(
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

