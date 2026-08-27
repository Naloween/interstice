use super::*;

/// The starting node(s) for a query: a scoped element's direct children (so the
/// scope element itself is excluded, matching `Element.querySelector`), or every
/// document root.
pub(crate) fn scope_roots(dom: &Dom, scope: Option<NodeId>) -> Vec<NodeId> {
    match scope {
        Some(id) => dom.node(id).children.clone(),
        None => dom.roots().to_vec(),
    }
}

/// Depth-first (document order) walk from `id`, invoking `hit(node)` on each match.
/// `hit` returns `true` to stop the whole walk early. `skip_self` is unused here
/// because [`scope_roots`] already excludes the scope element; roots are always
/// eligible.
pub(crate) fn find_match(
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
pub(crate) fn find_by_id(dom: &Dom, id: NodeId, want: &str) -> Option<NodeId> {
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
pub(crate) fn find_first_tag(dom: &Dom, tag: &str) -> Option<NodeId> {
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

/// How an `[attr…]` clause compares: presence, or one of the value operators.
pub(crate) enum AttrOp {
    /// `[attr]` — the attribute is present (any value).
    Exists,
    /// `[attr=val]` — exact match.
    Eq,
    /// `[attr~=val]` — whitespace-separated list contains `val`.
    Include,
    /// `[attr|=val]` — equals `val`, or begins with `val-`.
    Dash,
    /// `[attr^=val]` — value begins with `val`.
    Prefix,
    /// `[attr$=val]` — value ends with `val`.
    Suffix,
    /// `[attr*=val]` — value contains `val`.
    Substr,
}

/// One `[attr op "value"]` clause on a compound selector.
pub(crate) struct AttrMatch {
    name: String,
    op: AttrOp,
    value: String,
}

impl AttrMatch {
    fn matches(&self, attr: Option<&str>) -> bool {
        let Some(v) = attr else {
            return false; // attribute absent → only nothing matches
        };
        match self.op {
            AttrOp::Exists => true,
            AttrOp::Eq => v == self.value,
            AttrOp::Include => v.split_whitespace().any(|p| p == self.value),
            AttrOp::Dash => v == self.value || v.starts_with(&format!("{}-", self.value)),
            AttrOp::Prefix => !self.value.is_empty() && v.starts_with(&self.value),
            AttrOp::Suffix => !self.value.is_empty() && v.ends_with(&self.value),
            AttrOp::Substr => !self.value.is_empty() && v.contains(&self.value),
        }
    }
}

/// A single compound selector: an optional type, an optional `#id`, any number of
/// `.class`es, and any number of `[attr]` clauses (e.g. `a#main.active[lang=en]`).
/// Combinators and grouping aren't modelled — a descendant selector like `nav a` is
/// reduced to its subject (`a`), matching the whole document, which over-matches but
/// never under-matches (the same pragmatic simplification the CSS cascade in
/// `css.rs` makes for combinators). Pseudo-classes (`:hover`, …) are ignored.
pub(crate) struct SimpleSelector {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
    attrs: Vec<AttrMatch>,
}

impl SimpleSelector {
    pub(crate) fn parse(input: &str) -> Option<SimpleSelector> {
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
            attrs: Vec::new(),
        };
        // Scan into runs delimited by `#` / `.` / `[` / `:`; the leading run (no
        // sigil) is the type. `kind`: '\0' = type, '#' = id, '.' = class, ':' = an
        // ignored pseudo-class.
        let mut kind = b'\0';
        let mut cur = String::new();
        let flush = |kind: u8, cur: &mut String, sel: &mut SimpleSelector| {
            if cur.is_empty() {
                return;
            }
            let val = std::mem::take(cur);
            match kind {
                b'#' => sel.id = Some(val),
                b'.' => sel.classes.push(val),
                b':' => {} // pseudo-class: ignored (over-matches, never under-matches)
                _ => {
                    if val != "*" {
                        sel.tag = Some(val.to_ascii_lowercase());
                    }
                }
            }
        };
        let chars: Vec<char> = subject.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let ch = chars[i];
            if ch == '[' {
                flush(kind, &mut cur, &mut sel);
                kind = b'\0';
                // Consume up to the matching `]`.
                let mut inner = String::new();
                i += 1;
                while i < chars.len() && chars[i] != ']' {
                    inner.push(chars[i]);
                    i += 1;
                }
                i += 1; // skip ']'
                if let Some(am) = parse_attr_match(&inner) {
                    sel.attrs.push(am);
                }
                continue;
            }
            if ch == '#' || ch == '.' || ch == ':' {
                flush(kind, &mut cur, &mut sel);
                kind = ch as u8;
            } else {
                cur.push(ch);
            }
            i += 1;
        }
        flush(kind, &mut cur, &mut sel);
        // Even an empty compound (a bare `*`) is a valid match-everything selector.
        Some(sel)
    }

    pub(crate) fn matches(&self, dom: &Dom, id: NodeId) -> bool {
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
        for am in &self.attrs {
            if !am.matches(node.attr(&am.name)) {
                return false;
            }
        }
        true
    }
}

/// Parse the inside of an `[…]` clause: `attr`, `attr=val`, or `attr <op>= val`,
/// where the value may be single/double-quoted. Returns `None` for an empty name.
pub(crate) fn parse_attr_match(inner: &str) -> Option<AttrMatch> {
    let inner = inner.trim();
    // Find the operator: one of ~ | ^ $ * immediately before `=`, or a bare `=`.
    let (name, op, rest) = if let Some(eq) = inner.find('=') {
        let (before, after) = inner.split_at(eq);
        let after = &after[1..]; // drop '='
        let (name, op) = if let Some(nm) = before.strip_suffix('~') {
            (nm, AttrOp::Include)
        } else if let Some(nm) = before.strip_suffix('|') {
            (nm, AttrOp::Dash)
        } else if let Some(nm) = before.strip_suffix('^') {
            (nm, AttrOp::Prefix)
        } else if let Some(nm) = before.strip_suffix('$') {
            (nm, AttrOp::Suffix)
        } else if let Some(nm) = before.strip_suffix('*') {
            (nm, AttrOp::Substr)
        } else {
            (before, AttrOp::Eq)
        };
        (name.trim(), op, after.trim())
    } else {
        (inner, AttrOp::Exists, "")
    };
    if name.is_empty() {
        return None;
    }
    // Strip surrounding quotes and a trailing case-insensitivity flag (` i`/` s`).
    let mut value = rest.trim();
    if value.ends_with(" i") || value.ends_with(" s") || value.ends_with(" I") || value.ends_with(" S") {
        value = value[..value.len() - 2].trim_end();
    }
    let value = value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
        .unwrap_or(value);
    Some(AttrMatch {
        name: name.to_string(),
        op,
        value: value.to_string(),
    })
}
