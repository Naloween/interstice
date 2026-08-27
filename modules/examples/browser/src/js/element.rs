use super::*;

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
pub(crate) fn node_key() -> boa_engine::JsString {
    js_string!("__node")
}

/// Read-only + enumerable — the attribute set for an element's scalar DOM props.
pub(crate) fn ro() -> Attribute {
    Attribute::READONLY | Attribute::ENUMERABLE
}

/// Recover the arena node id from a method's `this`, or `None` if `this` isn't an
/// element object we built (e.g. the method was `call`-ed on something else).
pub(crate) fn this_node(this: &JsValue, ctx: &mut Context) -> Option<NodeId> {
    let obj = this.as_object()?;
    let v = obj.get(node_key(), ctx).ok()?;
    Some(v.to_u32(ctx).ok()? as NodeId)
}

/// The `idx`-th argument coerced to a Rust string (empty if absent/uncoercible).
pub(crate) fn arg_string(args: &[JsValue], idx: usize, ctx: &mut Context) -> String {
    args.get(idx)
        .and_then(|v| v.to_string(ctx).ok())
        .map(|s| s.to_std_string_escaped())
        .unwrap_or_default()
}

/// The `idx`-th argument interpreted as one of our element objects → its arena id.
pub(crate) fn arg_node(args: &[JsValue], idx: usize, ctx: &mut Context) -> Option<NodeId> {
    let obj = args.get(idx)?.as_object()?;
    let v = obj.get(node_key(), ctx).ok()?;
    Some(v.to_u32(ctx).ok()? as NodeId)
}

/// Wrap a native fn pointer as a `JsFunction` in `ctx`'s realm — for accessors,
/// which (unlike methods on `ObjectInitializer`) take `JsFunction`s directly.
pub(crate) fn js_fn(ctx: &Context, ptr: NativeFunctionPointer) -> JsFunction {
    NativeFunction::from_fn_ptr(ptr).to_js_function(ctx.realm())
}

/// Build a JS element object wrapping arena node `id`. Structural and textual
/// properties are **live accessors** (they read/write the arena on each access), so
/// a handle stays correct across mutations; identity is the hidden `__node` id.
pub(crate) fn element_object(ctx: &mut Context, id: NodeId) -> JsValue {
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
    let get_parent_node = js_fn(ctx, el_get_parent_element);
    let get_style = js_fn(ctx, el_get_style);
    let get_value = js_fn(ctx, el_get_value);
    let set_value = js_fn(ctx, el_set_value);

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
        .accessor(js_string!("parentNode"), Some(get_parent_node), None, ro())
        .accessor(js_string!("children"), Some(get_children), None, ro())
        .accessor(js_string!("firstElementChild"), Some(get_first), None, ro())
        .accessor(js_string!("nextElementSibling"), Some(get_next), None, ro())
        .accessor(js_string!("classList"), Some(get_classlist), None, ro())
        // Inline style, and the form-control `value`. `style` is a live-ish object
        // seeded from the element's inline `style` attribute; writes to individual
        // properties don't (yet) flow back to layout, but reads/writes don't throw.
        .accessor(js_string!("style"), Some(get_style), None, ro())
        .accessor(js_string!("value"), Some(get_value), Some(set_value), ro())
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
            NativeFunction::from_fn_ptr(el_matches),
            js_string!("matches"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(el_closest),
            js_string!("closest"),
            1,
        )
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
        // Focus management and scrolling are no-ops (we have no focus model or a
        // scroll offset a script can drive), but pages call them at load — e.g. the
        // portal focuses the search box — so they must exist and be callable.
        .function(NativeFunction::from_fn_ptr(el_noop), js_string!("focus"), 0)
        .function(NativeFunction::from_fn_ptr(el_noop), js_string!("blur"), 0)
        .function(
            NativeFunction::from_fn_ptr(el_noop),
            js_string!("scrollIntoView"),
            0,
        )
        .function(NativeFunction::from_fn_ptr(el_noop), js_string!("click"), 0)
        .build();
    obj.into()
}

/// A callable no-op for DOM methods we don't model (focus/blur/scroll/…). Returns
/// `undefined`; exists only so `el.focus()` etc. don't throw "not a callable".
pub(crate) fn el_noop(_t: &JsValue, _a: &[JsValue], _c: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::undefined())
}

// Accessor getters/setters. Getters ignore args; setters take the new value as
// `args[0]`. All return `undefined` on a bad `this` rather than throwing.

pub(crate) fn el_get_tag_name(this: &JsValue, _a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::undefined());
    };
    let tag = crate::dom::with(|d| d.node(id).tag().unwrap_or_default().to_ascii_uppercase());
    Ok(js_string!(tag).into())
}

pub(crate) fn el_get_id(this: &JsValue, _a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    Ok(js_string!(attr_of(this, ctx, "id")).into())
}

pub(crate) fn el_set_id(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    set_attr_from(this, args, ctx, "id");
    Ok(JsValue::undefined())
}

pub(crate) fn el_get_class_name(this: &JsValue, _a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    Ok(js_string!(attr_of(this, ctx, "class")).into())
}

/// `element.value` — for form controls (`<input>`/`<select>`/`<textarea>`), backed
/// by the `value` attribute (a plain string). Returns "" when unset so reads never
/// yield `undefined`.
pub(crate) fn el_get_value(this: &JsValue, _a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    Ok(js_string!(attr_of(this, ctx, "value")).into())
}

pub(crate) fn el_set_value(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    set_attr_from(this, args, ctx, "value");
    Ok(JsValue::undefined())
}

/// `element.style` — a fresh object seeded from the element's inline `style`
/// attribute, exposing each declared property in camelCase (`padding-right` →
/// `paddingRight`) plus a `cssText`. It's writable so `el.style.foo = "…"` doesn't
/// throw, but writes are not yet reflected back into layout — enough to keep scripts
/// running (e.g. the portal padding tweak) without a full CSSOM.
pub(crate) fn el_get_style(this: &JsValue, _a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let css = match this_node(this, ctx) {
        Some(id) => crate::dom::with(|d| d.node(id).attr("style").unwrap_or_default().to_string()),
        None => String::new(),
    };
    let mut init = ObjectInitializer::new(ctx);
    init.property(js_string!("cssText"), js_string!(css.clone()), rw());
    for decl in css.split(';') {
        let Some((prop, val)) = decl.split_once(':') else {
            continue;
        };
        let prop = prop.trim();
        let val = val.trim();
        if prop.is_empty() {
            continue;
        }
        // kebab-case → camelCase (border-radius → borderRadius).
        let mut camel = String::new();
        let mut upper = false;
        for ch in prop.chars() {
            if ch == '-' {
                upper = true;
            } else if upper {
                camel.extend(ch.to_ascii_uppercase().to_string().chars());
                upper = false;
            } else {
                camel.push(ch);
            }
        }
        init.property(js_string!(camel), js_string!(val.to_string()), rw());
    }
    Ok(init.build().into())
}

pub(crate) fn el_set_class_name(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    set_attr_from(this, args, ctx, "class");
    Ok(JsValue::undefined())
}

pub(crate) fn el_get_text_content(this: &JsValue, _a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::undefined());
    };
    let text = crate::dom::with(|d| d.inner_text(id));
    Ok(js_string!(text).into())
}

pub(crate) fn el_set_text_content(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::undefined());
    };
    let text = arg_string(args, 0, ctx);
    crate::dom::with_mut(|d| d.set_text_content(id, &text));
    Ok(JsValue::undefined())
}

pub(crate) fn el_get_inner_html(this: &JsValue, _a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
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

pub(crate) fn el_set_inner_html(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::undefined());
    };
    let html = arg_string(args, 0, ctx);
    crate::dom::with_mut(|d| d.set_inner_html(id, &html));
    Ok(JsValue::undefined())
}

pub(crate) fn el_get_outer_html(this: &JsValue, _a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::undefined());
    };
    let html = crate::dom::with(|d| d.serialize(id));
    Ok(js_string!(html).into())
}

pub(crate) fn el_get_parent_element(this: &JsValue, _a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
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

pub(crate) fn el_get_children(this: &JsValue, _a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
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

pub(crate) fn el_get_first_element_child(
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

pub(crate) fn el_get_next_element_sibling(
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

pub(crate) fn el_set_attribute(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::undefined());
    };
    let name = arg_string(args, 0, ctx);
    let value = arg_string(args, 1, ctx);
    crate::dom::with_mut(|d| d.set_attr(id, &name, &value));
    Ok(JsValue::undefined())
}

pub(crate) fn el_remove_attribute(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::undefined());
    };
    let name = arg_string(args, 0, ctx);
    crate::dom::with_mut(|d| d.remove_attr(id, &name));
    Ok(JsValue::undefined())
}

pub(crate) fn el_append_child(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let (Some(parent), Some(child)) = (this_node(this, ctx), arg_node(args, 0, ctx)) else {
        return Ok(JsValue::undefined());
    };
    crate::dom::with_mut(|d| d.append_child(parent, child));
    Ok(args.first().cloned().unwrap_or_default())
}

pub(crate) fn el_insert_before(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let (Some(parent), Some(child)) = (this_node(this, ctx), arg_node(args, 0, ctx)) else {
        return Ok(JsValue::undefined());
    };
    let reference = arg_node(args, 1, ctx);
    crate::dom::with_mut(|d| d.insert_before(parent, child, reference));
    Ok(args.first().cloned().unwrap_or_default())
}

pub(crate) fn el_remove_child(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let (Some(parent), Some(child)) = (this_node(this, ctx), arg_node(args, 0, ctx)) else {
        return Ok(JsValue::undefined());
    };
    crate::dom::with_mut(|d| d.remove_child(parent, child));
    Ok(args.first().cloned().unwrap_or_default())
}

pub(crate) fn el_replace_child(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
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

pub(crate) fn el_remove(this: &JsValue, _a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
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

// ── classList ────────────────────────────────────────────────────────────────

/// `element.classList` — a fresh token-list object over the element's `class`
/// attribute (carrying the same hidden `__node`), with add/remove/toggle/contains.
pub(crate) fn el_get_class_list(this: &JsValue, _a: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
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

pub(crate) fn class_tokens(id: NodeId) -> Vec<String> {
    crate::dom::with(|d| {
        d.node(id)
            .attr("class")
            .unwrap_or_default()
            .split_whitespace()
            .map(str::to_string)
            .collect()
    })
}

pub(crate) fn write_class_tokens(id: NodeId, tokens: &[String]) {
    crate::dom::with_mut(|d| d.set_attr(id, "class", &tokens.join(" ")));
}

pub(crate) fn cl_add(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
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

pub(crate) fn cl_remove(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
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

pub(crate) fn cl_toggle(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
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

pub(crate) fn cl_contains(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::from(false));
    };
    let name = arg_string(args, 0, ctx);
    Ok(JsValue::from(class_tokens(id).iter().any(|t| *t == name)))
}

pub(crate) fn el_get_attribute(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
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

pub(crate) fn el_has_attribute(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::from(false));
    };
    let name = arg_string(args, 0, ctx).to_ascii_lowercase();
    let has = crate::dom::with(|dom| dom.node(id).attr(&name).is_some());
    Ok(JsValue::from(has))
}

/// `element.querySelector` — first descendant of `this` matching the selector.
pub(crate) fn el_query_selector(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsValue::null());
    };
    let sel = arg_string(args, 0, ctx);
    query_first(ctx, Some(id), &sel)
}

pub(crate) fn el_query_selector_all(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = this_node(this, ctx) else {
        return Ok(JsArray::new(ctx).into());
    };
    let sel = arg_string(args, 0, ctx);
    query_all(ctx, Some(id), &sel)
}

pub(crate) fn el_get_elements_by_tag_name(
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
