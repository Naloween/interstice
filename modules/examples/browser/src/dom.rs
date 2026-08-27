//! The persistent DOM arena: the browser's own mutable document tree, and the
//! single source of truth for rendering. It is built once from a `tl` parse when
//! a document arrives and stored in a `thread_local`, so it survives across
//! reducer calls (parsed in `on_http`, re-rendered later in `on_frame`/CSS
//! responses). The HTML walker ([`crate::html`]) reads from here instead of
//! walking `tl` directly, so once the JS phases land, a script's DOM mutation
//! flows straight into the next render with no re-parse.
//!
//! Storing the live tree in linear memory (rather than tables) is deliberate: a
//! script reads its own writes synchronously within one reducer run, which the
//! table write-visibility rules forbid, and node access is the hottest path in a
//! page's life. Its *output* — the `UiElement` rows — stays table-backed. This is
//! the read/write sibling of the `OnceLock` font caches graphics already keeps in
//! linear memory.

use std::cell::{Cell, RefCell};

/// An index into [`Dom::nodes`]. Stable for a node's lifetime within one document.
pub type NodeId = usize;

/// What a node is. `tl` performs no entity decoding, so every string here is raw
/// source text (e.g. `&amp;` stays literal) — matching what the walker expects
/// (it decodes text itself) and letting [`Dom::serialize`] reproduce source.
pub enum NodeKind {
    Element {
        /// Lowercased tag name.
        tag: String,
        /// Attributes in roughly source order (name lowercased, raw value). A
        /// value-less attribute (`disabled`) stores an empty string. Order is kept
        /// so an inline `<svg>` re-serializes into valid source.
        attrs: Vec<(String, String)>,
    },
    Text(String),
    Comment(String),
}

pub struct Node {
    pub kind: NodeKind,
    pub children: Vec<NodeId>,
    pub parent: Option<NodeId>,
}

impl Node {
    /// The lowercased tag name, or `None` for text/comment nodes.
    pub fn tag(&self) -> Option<&str> {
        match &self.kind {
            NodeKind::Element { tag, .. } => Some(tag),
            _ => None,
        }
    }

    /// The raw value of attribute `key` (lowercase), or `None` if absent / not an
    /// element. A value-less attribute yields `Some("")`.
    pub fn attr(&self, key: &str) -> Option<&str> {
        match &self.kind {
            NodeKind::Element { attrs, .. } => attrs
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str()),
            _ => None,
        }
    }

}

pub struct Dom {
    pub nodes: Vec<Node>,
    /// Top-level nodes, in document order.
    pub roots: Vec<NodeId>,
}

impl Dom {
    fn empty() -> Self {
        Dom {
            nodes: Vec::new(),
            roots: Vec::new(),
        }
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id]
    }

    pub fn roots(&self) -> &[NodeId] {
        &self.roots
    }

    fn push(&mut self, node: Node) -> NodeId {
        let id = self.nodes.len();
        self.nodes.push(node);
        id
    }

    /// The concatenated raw text of a node's whole subtree (the `<style>`/`<script>`
    /// content path — those are raw-text elements with a single text child).
    pub fn inner_text(&self, id: NodeId) -> String {
        let mut out = String::new();
        self.collect_text(id, &mut out);
        out
    }

    fn collect_text(&self, id: NodeId, out: &mut String) {
        match &self.node(id).kind {
            NodeKind::Text(t) => out.push_str(t),
            NodeKind::Element { .. } => {
                for &c in &self.node(id).children {
                    self.collect_text(c, out);
                }
            }
            NodeKind::Comment(_) => {}
        }
    }

    /// Re-serialize a subtree back to markup (the inline-`<svg>` path: usvg parses
    /// the result). Values/text are emitted verbatim because they are already raw
    /// source; attribute order and quote style may differ from the original but the
    /// output is well-formed. Used only for self-contained SVG islands.
    pub fn serialize(&self, id: NodeId) -> String {
        let mut out = String::new();
        self.serialize_into(id, &mut out);
        out
    }

    fn serialize_into(&self, id: NodeId, out: &mut String) {
        match &self.node(id).kind {
            NodeKind::Text(t) => out.push_str(t),
            NodeKind::Comment(c) => {
                out.push_str("<!--");
                out.push_str(c);
                out.push_str("-->");
            }
            NodeKind::Element { tag, attrs } => {
                out.push('<');
                out.push_str(tag);
                for (k, v) in attrs {
                    out.push(' ');
                    out.push_str(k);
                    out.push_str("=\"");
                    out.push_str(v);
                    out.push('"');
                }
                out.push('>');
                for &c in &self.node(id).children {
                    self.serialize_into(c, out);
                }
                out.push_str("</");
                out.push_str(tag);
                out.push('>');
            }
        }
    }

    // ── Mutation (Phase 4) ───────────────────────────────────────────────────
    //
    // The arena is append-only for *storage*: nodes are never removed from
    // `nodes`, only re-parented or orphaned. So a `NodeId` handed to JS stays valid
    // for the page's life even after the node is detached — matching how a real DOM
    // keeps a removed node alive as long as a script references it.

    /// Detach `child` from its current parent (if any), leaving it orphaned.
    fn detach(&mut self, child: NodeId) {
        if let Some(p) = self.nodes[child].parent {
            self.nodes[p].children.retain(|&c| c != child);
        }
        self.nodes[child].parent = None;
    }

    /// Set attribute `name` to `value` on an element (created if absent). No-op on
    /// text/comment nodes.
    pub fn set_attr(&mut self, id: NodeId, name: &str, value: &str) {
        if let NodeKind::Element { attrs, .. } = &mut self.nodes[id].kind {
            let name = name.to_ascii_lowercase();
            if let Some(slot) = attrs.iter_mut().find(|(k, _)| *k == name) {
                slot.1 = value.to_string();
            } else {
                attrs.push((name, value.to_string()));
            }
        }
    }

    /// Remove attribute `name` from an element, if present.
    pub fn remove_attr(&mut self, id: NodeId, name: &str) {
        if let NodeKind::Element { attrs, .. } = &mut self.nodes[id].kind {
            let name = name.to_ascii_lowercase();
            attrs.retain(|(k, _)| *k != name);
        }
    }

    /// Create a detached element node and return its id.
    pub fn create_element(&mut self, tag: &str) -> NodeId {
        self.push(Node {
            kind: NodeKind::Element {
                tag: tag.to_ascii_lowercase(),
                attrs: Vec::new(),
            },
            children: Vec::new(),
            parent: None,
        })
    }

    /// Create a detached text node and return its id.
    pub fn create_text(&mut self, text: &str) -> NodeId {
        self.push(Node {
            kind: NodeKind::Text(text.to_string()),
            children: Vec::new(),
            parent: None,
        })
    }

    /// `element.textContent = text`: replace an element's whole subtree with a
    /// single text node; or, on a text node, replace its data directly.
    pub fn set_text_content(&mut self, id: NodeId, text: &str) {
        match &self.nodes[id].kind {
            NodeKind::Text(_) => self.nodes[id].kind = NodeKind::Text(text.to_string()),
            NodeKind::Element { .. } => {
                let old = std::mem::take(&mut self.nodes[id].children);
                for c in old {
                    self.nodes[c].parent = None;
                }
                if !text.is_empty() {
                    let t = self.create_text(text);
                    self.nodes[t].parent = Some(id);
                    self.nodes[id].children.push(t);
                }
            }
            NodeKind::Comment(_) => {}
        }
    }

    /// `parent.appendChild(child)`: move `child` to be `parent`'s last child.
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        self.detach(child);
        self.nodes[child].parent = Some(parent);
        self.nodes[parent].children.push(child);
    }

    /// `parent.insertBefore(child, reference)`: insert before `reference` (a current
    /// child of `parent`), or append if `reference` is `None`/not found.
    pub fn insert_before(&mut self, parent: NodeId, child: NodeId, reference: Option<NodeId>) {
        self.detach(child);
        self.nodes[child].parent = Some(parent);
        let pos = reference
            .and_then(|r| self.nodes[parent].children.iter().position(|&c| c == r))
            .unwrap_or(self.nodes[parent].children.len());
        self.nodes[parent].children.insert(pos, child);
    }

    /// `parent.removeChild(child)`: detach `child` only if it is really a child.
    pub fn remove_child(&mut self, parent: NodeId, child: NodeId) {
        if self.nodes[child].parent == Some(parent) {
            self.detach(child);
        }
    }

    /// `element.innerHTML = html`: replace children with the parse of `html`.
    pub fn set_inner_html(&mut self, id: NodeId, html: &str) {
        let old = std::mem::take(&mut self.nodes[id].children);
        for c in old {
            self.nodes[c].parent = None;
        }
        let parsed = match tl::parse(html, tl::ParserOptions::default()) {
            Ok(d) => d,
            Err(_) => return,
        };
        let parser = parsed.parser();
        for handle in parsed.children() {
            if let Some(cid) = build_node(self, parser, *handle, Some(id)) {
                self.nodes[id].children.push(cid);
            }
        }
    }
}

thread_local! {
    /// The current document's arena. Replaced wholesale on each new page load.
    static DOM: RefCell<Dom> = RefCell::new(Dom::empty());
    /// Set by [`with_mut`] whenever the arena is mutated. `on_frame` consumes it via
    /// [`take_dirty`] after dispatching a JS event: a `true` result means a handler
    /// changed the DOM and the page must be re-rendered from the live arena. Cleared
    /// at the end of every full `render::rebuild` (which already reflects the arena).
    static DIRTY: Cell<bool> = const { Cell::new(false) };
}

/// Build the arena from `html` and store it as the current document. Called once,
/// when the main document response arrives, replacing the previous page's tree.
pub fn set(html: &str) {
    let dom = build(html);
    DOM.with(|d| *d.borrow_mut() = dom);
}

/// Run `f` against the current document arena (read-only). Both the stylesheet
/// collector and the block walker read the tree this way.
pub fn with<R>(f: impl FnOnce(&Dom) -> R) -> R {
    DOM.with(|d| f(&d.borrow()))
}

/// Run `f` against the current arena with mutable access, marking the arena dirty.
/// Every JS DOM mutation goes through here. Inline scripts run before the page's
/// (possibly CSS-deferred) build, so their mutations are already reflected on first
/// paint (and the dirty flag they set is cleared when that build finishes); an
/// event handler mutating *after* render leaves the flag set so `on_frame`
/// re-renders.
pub fn with_mut<R>(f: impl FnOnce(&mut Dom) -> R) -> R {
    let r = DOM.with(|d| f(&mut d.borrow_mut()));
    DIRTY.with(|dirty| dirty.set(true));
    r
}

/// Take and clear the "arena mutated since the last render" flag.
pub fn take_dirty() -> bool {
    DIRTY.with(|d| d.replace(false))
}

/// Two control-character sentinels that stand in for `<` and `>` inside a rawtext
/// element's body while `tl` parses. They never occur in real source, so the
/// substitution is exact and reversible (see [`neutralize_rawtext`]).
const LT_SENTINEL: char = '\u{1}';
const GT_SENTINEL: char = '\u{2}';

/// HTML rawtext / RCDATA elements, whose content is text, never markup.
const RAWTEXT_TAGS: [&str; 4] = ["script", "style", "textarea", "title"];

/// Neutralize `<`/`>` inside rawtext element bodies before parsing.
///
/// `tl` 0.7.8 does not treat `<script>` (or `<style>`) as a rawtext element, so a
/// bare `<` in an inline script's minified JS — `for(;e<b;)` — is misparsed as a
/// tag start. On a real page (google.com) that cascades every following node,
/// including the actual `<body>`, deep inside `<head>`, and the walker then emits
/// nothing. Browsers tokenize these elements as rawtext: their content runs
/// verbatim until the matching `</tag>`. We reproduce that by replacing `<`/`>`
/// in each rawtext body with sentinels tl can't trip on; [`build_node`] restores
/// them in the text node it builds, so script/style/title text round-trips exactly.
fn neutralize_rawtext(html: &str) -> String {
    let b = html.as_bytes();
    let lower = html.to_ascii_lowercase();
    let lb = lower.as_bytes();
    let mut out = String::with_capacity(html.len());
    // Start of the run of verbatim source not yet flushed to `out`.
    let mut seg_start = 0;
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'<' {
            let name_start = i + 1;
            let tag = RAWTEXT_TAGS.iter().copied().find(|tag| {
                lb[name_start..].starts_with(tag.as_bytes()) && {
                    // The name must be followed by a tag-name terminator so we don't
                    // match `<scripter>` on `script`.
                    let after = *lb.get(name_start + tag.len()).unwrap_or(&b'>');
                    matches!(after, b' ' | b'>' | b'/' | b'\t' | b'\n' | b'\r')
                }
            });
            if let Some(tag) = tag {
                // Find the end of the opening tag (`>`), skipping quoted attributes.
                let mut j = name_start;
                let mut quote = 0u8;
                while j < b.len() {
                    let c = b[j];
                    if quote != 0 {
                        if c == quote {
                            quote = 0;
                        }
                    } else if c == b'"' || c == b'\'' {
                        quote = c;
                    } else if c == b'>' {
                        break;
                    }
                    j += 1;
                }
                let open_end = (j + 1).min(b.len());
                // The body runs to the matching `</tag`; an unterminated element
                // (no close tag) runs to end of input, exactly like a browser.
                let close_pat = format!("</{tag}");
                let body_end = lower[open_end..]
                    .find(&close_pat)
                    .map(|r| open_end + r)
                    .unwrap_or(b.len());
                // Flush verbatim source up to the body, then the neutralized body.
                out.push_str(&html[seg_start..open_end]);
                for ch in html[open_end..body_end].chars() {
                    match ch {
                        '<' => out.push(LT_SENTINEL),
                        '>' => out.push(GT_SENTINEL),
                        c => out.push(c),
                    }
                }
                seg_start = body_end;
                i = body_end;
                continue;
            }
        }
        i += 1;
    }
    out.push_str(&html[seg_start..]);
    out
}

/// Parse `html` with `tl` and copy it into an owned arena. Invalid HTML yields an
/// empty document.
fn build(html: &str) -> Dom {
    let mut dom = Dom::empty();
    let neutralized = neutralize_rawtext(html);
    let parsed = match tl::parse(&neutralized, tl::ParserOptions::default()) {
        Ok(d) => d,
        Err(_) => return dom,
    };
    let parser = parsed.parser();
    for handle in parsed.children() {
        if let Some(id) = build_node(&mut dom, parser, *handle, None) {
            dom.roots.push(id);
        }
    }
    dom
}

fn build_node(
    dom: &mut Dom,
    parser: &tl::Parser,
    handle: tl::NodeHandle,
    parent: Option<NodeId>,
) -> Option<NodeId> {
    let node = handle.get(parser)?;
    match node {
        tl::Node::Raw(bytes) => {
            let mut text = bytes.as_utf8_str().into_owned();
            // Undo the rawtext neutralization (see `neutralize_rawtext`): the
            // sentinels only ever mark a script/style body's original `<`/`>`.
            if text.contains(LT_SENTINEL) || text.contains(GT_SENTINEL) {
                text = text.replace(LT_SENTINEL, "<").replace(GT_SENTINEL, ">");
            }
            Some(dom.push(Node {
                kind: NodeKind::Text(text),
                children: Vec::new(),
                parent,
            }))
        }
        tl::Node::Comment(bytes) => {
            // Strip the `<!--`/`-->` wrapper `tl` keeps so serialize can re-add it.
            let raw = bytes.as_utf8_str();
            let inner = raw
                .strip_prefix("<!--")
                .and_then(|s| s.strip_suffix("-->"))
                .unwrap_or(&raw)
                .to_string();
            Some(dom.push(Node {
                kind: NodeKind::Comment(inner),
                children: Vec::new(),
                parent,
            }))
        }
        tl::Node::Tag(tag) => {
            let name = tag.name().as_utf8_str().to_ascii_lowercase();
            let attrs = tag
                .attributes()
                .iter()
                .map(|(k, v)| {
                    (
                        k.to_ascii_lowercase(),
                        v.map(|c| c.into_owned()).unwrap_or_default(),
                    )
                })
                .collect();
            let id = dom.push(Node {
                kind: NodeKind::Element { tag: name, attrs },
                children: Vec::new(),
                parent,
            });
            let kids: Vec<tl::NodeHandle> = tag.children().top().iter().copied().collect();
            for k in kids {
                if let Some(cid) = build_node(dom, parser, k, Some(id)) {
                    dom.nodes[id].children.push(cid);
                }
            }
            Some(id)
        }
    }
}
