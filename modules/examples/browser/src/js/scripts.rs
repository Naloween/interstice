use super::*;

/// One `console` method: stringify every argument, join with spaces, and forward
/// the line to the host log prefixed with its level. Shared by log/info/warn/error
/// via a captured `&'static str` level (a `Copy` closure, so `from_copy_closure`
/// is sound — it captures no traceable GC values).
pub(crate) fn console_method(
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
pub(crate) fn install_console(ctx: &mut Context) {
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
/// evaluating each in the current realm. External (`src`) scripts are fetched by
/// the page loader and run later via [`run_external_scripts`]. A script that throws
/// is logged and does not abort the rest — matching browser behaviour, where one
/// script's error doesn't stop later ones. Call after [`reset`] and after the DOM
/// arena for the page has been built.
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
pub(crate) fn collect_inline_scripts(dom: &crate::dom::Dom) -> Vec<String> {
    let mut out = Vec::new();
    for &root in dom.roots() {
        collect_scripts_from(dom, root, &mut out);
    }
    out
}

pub(crate) fn collect_scripts_from(dom: &crate::dom::Dom, id: crate::dom::NodeId, out: &mut Vec<String>) {
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

/// Collect the resolved-later `src` of every **external** executable `<script>` in
/// document order (the counterpart to [`collect_inline_scripts`]). The page loader
/// fetches these and hands their bodies back to [`run_external_scripts`]. Non-JS
/// `type`s (JSON, importmap) are skipped, matching inline-script handling.
pub(crate) fn collect_external_scripts() -> Vec<String> {
    crate::dom::with(|dom| {
        let mut out = Vec::new();
        for &root in dom.roots() {
            collect_external_from(dom, root, &mut out);
        }
        out
    })
}

pub(crate) fn collect_external_from(dom: &crate::dom::Dom, id: crate::dom::NodeId, out: &mut Vec<String>) {
    let node = dom.node(id);
    if node.tag() == Some("script") {
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
        if executable {
            if let Some(src) = node.attr("src") {
                let s = src.trim().to_string();
                if !s.is_empty() {
                    out.push(s);
                }
            }
        }
        return;
    }
    for &child in &node.children {
        collect_external_from(dom, child, out);
    }
}

/// Evaluate fetched external `<script>` bodies in document order against the
/// current realm, then drain microtasks — the same execution model as
/// [`run_inline_scripts`], just deferred until the fetches complete. A script that
/// throws is logged and doesn't abort the rest. Returns whether any script mutated
/// the DOM (the arena dirty flag), so the caller can re-render.
pub(crate) fn run_external_scripts(codes: &[String]) {
    if codes.is_empty() {
        return;
    }
    JS.with(|cell| {
        let mut guard = cell.borrow_mut();
        let Some(ctx) = guard.as_mut() else {
            return;
        };
        for code in codes {
            if let Err(e) = ctx.eval(Source::from_bytes(code)) {
                interstice_sdk::host_calls::log(&format!("js: script error: {e}"));
            }
        }
        if let Err(e) = ctx.run_jobs() {
            interstice_sdk::host_calls::log(&format!("js: microtask error: {e}"));
        }
    });
    // Scripts have run and registered their handlers; fire the readiness events that
    // `window.addEventListener("DOMContentLoaded"/"load", …)` code waits on.
    fire_document_ready();
}
