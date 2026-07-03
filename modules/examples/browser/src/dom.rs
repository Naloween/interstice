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

/// Parse `html` with `tl` and copy it into an owned arena. Invalid HTML yields an
/// empty document.
fn build(html: &str) -> Dom {
    let mut dom = Dom::empty();
    let parsed = match tl::parse(html, tl::ParserOptions::default()) {
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
        tl::Node::Raw(bytes) => Some(dom.push(Node {
            kind: NodeKind::Text(bytes.as_utf8_str().into_owned()),
            children: Vec::new(),
            parent,
        })),
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
