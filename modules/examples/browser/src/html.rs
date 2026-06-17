//! HTML → flat render-block translation. We parse with `tl`, walk the DOM, and
//! flatten it into a linear list of [`Block`]s (the document flow). Each element's
//! computed style comes from the cascade: a built-in UA layer ([`style`]) under
//! the page's author CSS ([`crate::css`]) under inline `style=""`. Inline runs
//! (plain text, links, colour-styled spans) are accumulated into ONE block-level
//! [`Block::Text`] per block element, carrying per-run [`Span`]s so the whole
//! paragraph wraps as a single flowing line box (links sit inline). Images stay
//! their own block. The caller turns blocks into `UiElement`s under the viewport.

use crate::css;
use crate::style::{self, Rgba, TextStyle};

/// An inline style run within a [`Block::Text`]: char range `[start, end)` over
/// the block's `text`, a colour override, and an optional link `href`.
pub struct Span {
    pub start: u32,
    pub end: u32,
    pub color: Rgba,
    pub href: Option<String>,
}

/// One laid-out piece of the document, in reading order.
pub enum Block {
    /// A block of flowing text. `color`/`size` are the block defaults; `spans`
    /// override colour and mark links for sub-ranges (links/inline-styled runs).
    /// `align` is the text-align factor (0 left / 0.5 centre / 1 right);
    /// `background` is an optional backdrop colour for the text box. `margin` and
    /// `padding` are per-side `(top, right, bottom, left)` px from the box model
    /// (the left margin already folds in any list/quote indent). `width` is the
    /// CSS width; `border_w`/`border_c` give an optional box border.
    Text {
        text: String,
        size: f32,
        color: Rgba,
        align: f32,
        background: Option<Rgba>,
        margin: (f32, f32, f32, f32),
        padding: (f32, f32, f32, f32),
        width: css::WidthVal,
        border_w: f32,
        border_c: Rgba,
        spans: Vec<Span>,
        /// `clear` set ⇒ this box drops below any preceding float instead of
        /// sitting beside it (ends the float context in [`group_floats`]).
        clears: bool,
    },
    /// An `<img>` to fetch + decode. `url` is as authored (resolved later).
    /// `float`/`clears` carry the CSS float context (see [`group_floats`]).
    Image {
        url: String,
        float: css::Float,
        clears: bool,
    },
    /// Vertical whitespace (paragraph spacing, `<br>`, `<hr>`).
    Space { height: f32 },
    /// A flex container (`display:flex`): its `children` lay out along the main
    /// axis per `direction`/`justify`/`align`, separated by `gap`. `margin`,
    /// `padding` are per-side `(t, r, b, l)`; `background` is an optional backdrop.
    /// Contiguous inline content among the children collapses into a single
    /// anonymous text child (one flex item), matching CSS anonymous flex items.
    Container {
        direction: css::FlexDirection,
        justify: css::Justify,
        align: css::Align,
        gap: f32,
        margin: (f32, f32, f32, f32),
        padding: (f32, f32, f32, f32),
        background: Option<Rgba>,
        children: Vec<Block>,
        /// `float` ⇒ this box is taken out of normal flow (see [`group_floats`]);
        /// `clears` ⇒ it drops below a preceding float.
        float: css::Float,
        clears: bool,
    },
    /// A float context produced by [`group_floats`]: `float_box` sits at the
    /// `side` (left/right) edge while `flow` (the following in-flow blocks)
    /// occupies the remaining width beside it. The beside-content keeps its
    /// narrowed column for its whole height (it does not reflow to full width
    /// below the float, unlike true CSS float).
    FloatRow {
        side: css::Float,
        float_box: Box<Block>,
        flow: Vec<Block>,
    },
}

struct State<'a> {
    /// Author cascade (UA defaults applied separately via [`style`]).
    sheet: &'a css::Stylesheet,
    /// Ancestor chain for descendant-selector matching (outermost first; the
    /// current element is not yet on the stack while it is being processed).
    stack: Vec<css::ElemCtx>,

    blocks: Vec<Block>,
    /// Accumulated text for the current block (whitespace already collapsed).
    buf: String,
    /// Style runs recorded for `buf` so far.
    spans: Vec<Span>,
    /// A whitespace boundary is pending: insert a single space before the next
    /// word so inline runs separated by whitespace don't run together, while
    /// adjacent runs with no whitespace (`<b>foo</b>bar`) stay joined.
    pending_space: bool,

    /// Current block's base style (size + base colour). `style.color` is the
    /// fallback the engine draws and the baseline against which a run gets a span.
    style: TextStyle,
    /// Current inline colour (differs from `style.color` inside coloured inline
    /// elements / links).
    cur_color: Rgba,
    /// Current inline link target (set inside `<a>`).
    cur_href: Option<String>,
    /// Current block's backdrop colour and text alignment.
    cur_bg: Option<Rgba>,
    cur_align: f32,
    indent: f32,
    /// Current block's box model: per-side margin/padding `(t, r, b, l)`, CSS
    /// width, and border. Reset per block (saved/restored around each block).
    cur_margin: (f32, f32, f32, f32),
    cur_padding: (f32, f32, f32, f32),
    cur_width: css::WidthVal,
    cur_border_w: f32,
    cur_border_c: Rgba,
    /// Current block's `clear`: drops the block below any preceding float.
    cur_clears: bool,
}

/// Parse a document against `sheet` and return its blocks. Invalid HTML yields
/// an empty list.
pub fn parse_html(html: &str, sheet: &css::Stylesheet) -> Vec<Block> {
    let dom = match tl::parse(html, tl::ParserOptions::default()) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    let parser = dom.parser();
    let mut st = State {
        sheet,
        stack: Vec::new(),
        blocks: Vec::new(),
        buf: String::new(),
        spans: Vec::new(),
        pending_space: false,
        style: TextStyle::body(),
        cur_color: TextStyle::body().color,
        cur_href: None,
        cur_bg: None,
        cur_align: 0.0,
        indent: 0.0,
        cur_margin: (0.0, 0.0, 0.0, 0.0),
        cur_padding: (0.0, 0.0, 0.0, 0.0),
        cur_width: css::WidthVal::Auto,
        cur_border_w: 0.0,
        cur_border_c: (0.0, 0.0, 0.0, 0.0),
        cur_clears: false,
    };
    for handle in dom.children() {
        walk(parser, *handle, &mut st);
    }
    flush(&mut st);
    st.blocks
}

/// The float side of a block (`None` for in-flow blocks).
fn block_float(b: &Block) -> css::Float {
    match b {
        Block::Image { float, .. } => *float,
        Block::Container { float, .. } => *float,
        _ => css::Float::None,
    }
}

/// Whether a block `clear`s — it must drop below a preceding float rather than
/// sit beside it, ending the float context.
fn block_clears(b: &Block) -> bool {
    match b {
        Block::Text { clears, .. } => *clears,
        Block::Image { clears, .. } => *clears,
        Block::Container { clears, .. } => *clears,
        _ => false,
    }
}

/// Post-pass over a flow's blocks: pair each `float`ed box with the in-flow
/// blocks that follow it (until a `clear` or the next float) into a
/// [`Block::FloatRow`], so the renderer can place the float at its edge with the
/// following content beside it. A lone float (no in-flow content after it) is
/// left as a normal block.
pub fn group_floats(blocks: Vec<Block>) -> Vec<Block> {
    let mut q: std::collections::VecDeque<Block> = blocks.into();
    let mut out = Vec::new();
    while let Some(b) = q.pop_front() {
        let side = block_float(&b);
        if side == css::Float::None {
            out.push(b);
            continue;
        }
        let mut flow = Vec::new();
        while let Some(next) = q.front() {
            if block_clears(next) || block_float(next) != css::Float::None {
                break;
            }
            flow.push(q.pop_front().unwrap());
        }
        if flow.is_empty() {
            out.push(b);
        } else {
            out.push(Block::FloatRow {
                side,
                float_box: Box::new(b),
                flow,
            });
        }
    }
    out
}

/// Scan a document for stylesheet sources: external `<link rel=stylesheet href>`
/// targets (resolved later) and inline `<style>` text, in source order.
pub fn collect_stylesheets(html: &str) -> (Vec<String>, Vec<String>) {
    let dom = match tl::parse(html, tl::ParserOptions::default()) {
        Ok(d) => d,
        Err(_) => return (Vec::new(), Vec::new()),
    };
    let parser = dom.parser();
    let mut links = Vec::new();
    let mut inline = Vec::new();
    for handle in dom.children() {
        collect_walk(parser, *handle, &mut links, &mut inline);
    }
    (links, inline)
}

fn collect_walk(
    parser: &tl::Parser,
    handle: tl::NodeHandle,
    links: &mut Vec<String>,
    inline: &mut Vec<String>,
) {
    let node = match handle.get(parser) {
        Some(n) => n,
        None => return,
    };
    if let tl::Node::Tag(tag) = node {
        let name = tag.name().as_utf8_str().to_ascii_lowercase();
        match name.as_str() {
            "link" => {
                let rel = tag
                    .attributes()
                    .get("rel")
                    .flatten()
                    .map(|b| b.as_utf8_str().to_ascii_lowercase())
                    .unwrap_or_default();
                if rel.split_whitespace().any(|t| t == "stylesheet") {
                    if let Some(Some(href)) = tag.attributes().get("href") {
                        let h = href.as_utf8_str().trim().to_string();
                        if !h.is_empty() {
                            links.push(h);
                        }
                    }
                }
            }
            "style" => {
                let css_text = tag.inner_text(parser).to_string();
                if !css_text.trim().is_empty() {
                    inline.push(css_text);
                }
            }
            _ => {}
        }
        let kids: Vec<tl::NodeHandle> = tag.children().top().iter().copied().collect();
        for k in kids {
            collect_walk(parser, k, links, inline);
        }
    }
}

fn walk(parser: &tl::Parser, handle: tl::NodeHandle, st: &mut State) {
    let node = match handle.get(parser) {
        Some(n) => n,
        None => return,
    };
    match node {
        tl::Node::Raw(bytes) => {
            push_run(st, &decode_entities(&bytes.as_utf8_str()));
        }
        tl::Node::Comment(_) => {}
        tl::Node::Tag(tag) => {
            let name = tag.name().as_utf8_str().to_ascii_lowercase();
            tag_node(parser, &name, tag, st);
        }
    }
}

/// Build the selector-matching context (tag/id/classes) for an element.
fn elem_ctx(name: &str, tag: &tl::HTMLTag) -> css::ElemCtx {
    let id = tag
        .attributes()
        .get("id")
        .flatten()
        .map(|b| b.as_utf8_str().trim().to_string())
        .filter(|s| !s.is_empty());
    let classes = tag
        .attributes()
        .get("class")
        .flatten()
        .map(|b| {
            b.as_utf8_str()
                .split_whitespace()
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default();
    css::ElemCtx {
        tag: name.to_string(),
        id,
        classes,
    }
}

/// The UA default `display` for a tag (author CSS may override it).
fn ua_display(name: &str) -> css::Display {
    if name == "a" || style::is_inline(name) {
        css::Display::Inline
    } else {
        css::Display::Block
    }
}

fn tag_node(parser: &tl::Parser, name: &str, tag: &tl::HTMLTag, st: &mut State) {
    if style::is_skipped(name) {
        return;
    }

    let ctx = elem_ctx(name, tag);
    let base_font = st.style.size;
    let mut applied = st.sheet.applied(&ctx, &st.stack, base_font);
    // Inline `style="…"` wins over author rules.
    if let Some(Some(style_attr)) = tag.attributes().get("style") {
        let inline = css::parse_inline(&style_attr.as_utf8_str(), base_font);
        applied.overlay(&inline);
    }

    let display = applied.display.unwrap_or_else(|| ua_display(name));
    if display == css::Display::None {
        return; // skip the whole subtree
    }

    // Replaced / void elements (handled regardless of display, aside from none).
    match name {
        "img" => {
            flush(st);
            if let Some(Some(src)) = tag.attributes().get("src") {
                let url = src.as_utf8_str().to_string();
                if !url.is_empty() {
                    st.blocks.push(Block::Image {
                        url,
                        float: applied.float.unwrap_or(css::Float::None),
                        clears: applied.clear.unwrap_or(false),
                    });
                }
            }
            return;
        }
        "br" => {
            flush(st);
            st.blocks.push(Block::Space { height: 6.0 });
            return;
        }
        "hr" => {
            flush(st);
            st.blocks.push(Block::Space { height: 14.0 });
            return;
        }
        "a" => {
            // Inline link: descend so nested formatting (and nested colours) is
            // preserved, tagging every run with this href + the link colour.
            let href = tag
                .attributes()
                .get("href")
                .flatten()
                .map(|b| b.as_utf8_str().to_string());
            let saved_color = st.cur_color;
            let saved_href = st.cur_href.clone();
            st.cur_color = applied.color.unwrap_or(style::LINK);
            st.cur_href = href;
            st.stack.push(ctx);
            walk_children(parser, tag, st);
            st.stack.pop();
            st.cur_color = saved_color;
            st.cur_href = saved_href;
            return;
        }
        _ => {}
    }

    if display == css::Display::Flex {
        // Flex container: collect its children into a nested block list instead
        // of flattening them into the document flow, so the renderer can lay
        // them out along the flex axis.
        flush(st);
        let saved_style = st.style;
        let saved_align = st.cur_align;
        let saved_color = st.cur_color;
        let saved_href = st.cur_href.clone();
        if let Some(c) = applied.color {
            st.style.color = c;
            st.cur_color = c;
        }
        if let Some(s) = applied.font_size {
            st.style.size = s;
        }
        if let Some(al) = applied.text_align {
            st.cur_align = al.factor();
        }

        // Divert child blocks into a fresh list, then restore the parent's.
        let saved_blocks = std::mem::take(&mut st.blocks);
        st.stack.push(ctx);
        walk_children(parser, tag, st);
        st.stack.pop();
        flush(st);
        let children = std::mem::replace(&mut st.blocks, saved_blocks);

        st.style = saved_style;
        st.cur_align = saved_align;
        st.cur_color = saved_color;
        st.cur_href = saved_href;

        if !children.is_empty() {
            st.blocks.push(Block::Container {
                direction: applied.flex_direction.unwrap_or(css::FlexDirection::Row),
                justify: applied.justify.unwrap_or(css::Justify::Start),
                align: applied.align.unwrap_or(css::Align::Stretch),
                gap: applied.gap.unwrap_or(0.0),
                margin: applied.margin_px(),
                padding: applied.padding_px(),
                background: applied.background,
                children,
                float: applied.float.unwrap_or(css::Float::None),
                clears: applied.clear.unwrap_or(false),
            });
        }
        return;
    }

    // Floated block (non-flex): a discrete column box pulled out of normal flow.
    // `group_floats` later places it beside the following in-flow content. Float
    // computes `display:block`, so this also catches floated inline elements.
    let float = applied.float.unwrap_or(css::Float::None);
    if float != css::Float::None {
        flush(st);
        let saved_style = st.style;
        let saved_align = st.cur_align;
        let saved_color = st.cur_color;
        let saved_href = st.cur_href.clone();
        if let Some(c) = applied.color {
            st.style.color = c;
            st.cur_color = c;
        }
        if let Some(s) = applied.font_size {
            st.style.size = s;
        }
        if let Some(al) = applied.text_align {
            st.cur_align = al.factor();
        }

        let saved_blocks = std::mem::take(&mut st.blocks);
        st.stack.push(ctx);
        walk_children(parser, tag, st);
        st.stack.pop();
        flush(st);
        let children = std::mem::replace(&mut st.blocks, saved_blocks);

        st.style = saved_style;
        st.cur_align = saved_align;
        st.cur_color = saved_color;
        st.cur_href = saved_href;

        if !children.is_empty() {
            st.blocks.push(Block::Container {
                direction: css::FlexDirection::Column,
                justify: css::Justify::Start,
                align: css::Align::Stretch,
                gap: 0.0,
                margin: applied.margin_px(),
                padding: applied.padding_px(),
                background: applied.background,
                children,
                float,
                clears: applied.clear.unwrap_or(false),
            });
        }
        return;
    }

    if matches!(display, css::Display::Inline | css::Display::InlineBlock) {
        // Inline element: fold children into the current run, optionally applying
        // a colour override (font-size on inline boxes isn't modelled yet).
        let saved_color = st.cur_color;
        if let Some(c) = applied.color {
            st.cur_color = c;
        }
        st.stack.push(ctx);
        walk_children(parser, tag, st);
        st.stack.pop();
        st.cur_color = saved_color;
        return;
    }

    // Block-level text element (p, h1, li, …): start a fresh styled block.
    if let Some(bs) = style::block_style(name, &st.style) {
        flush(st);
        if bs.space_before > 0.0 {
            st.blocks.push(Block::Space {
                height: bs.space_before,
            });
        }
        let saved_style = st.style;
        let saved_indent = st.indent;
        let saved_align = st.cur_align;
        let saved_bg = st.cur_bg;
        let saved_color = st.cur_color;
        let saved_href = st.cur_href.clone();
        let saved_margin = st.cur_margin;
        let saved_padding = st.cur_padding;
        let saved_width = st.cur_width;
        let saved_border_w = st.cur_border_w;
        let saved_border_c = st.cur_border_c;
        let saved_clears = st.cur_clears;

        st.style = bs;
        st.style.size = applied.font_size.unwrap_or(bs.size);
        st.style.color = applied.color.unwrap_or(bs.color);
        st.cur_color = st.style.color;
        st.cur_href = None;
        st.cur_bg = applied.background;
        if let Some(al) = applied.text_align {
            st.cur_align = al.factor();
        }
        st.indent += bs.indent_step;

        // Box model: CSS margin/padding/width/border for this block. The left
        // margin folds in the accumulated list/quote indent so both stack.
        let (mt, mr, mb, ml) = applied.margin_px();
        st.cur_margin = (mt, mr, mb, ml + st.indent);
        st.cur_padding = applied.padding_px();
        st.cur_width = applied.width.unwrap_or(css::WidthVal::Auto);
        st.cur_border_w = applied.border_width.unwrap_or(0.0);
        // `border-color` defaults to the text colour (CSS currentColor).
        st.cur_border_c = applied.border_color.unwrap_or(st.style.color);
        st.cur_clears = applied.clear.unwrap_or(false);

        if name == "li" {
            push_run(st, "• "); // bullet inherits the block colour ⇒ no span
        }
        st.stack.push(ctx);
        walk_children(parser, tag, st);
        st.stack.pop();
        flush(st);

        st.style = saved_style;
        st.indent = saved_indent;
        st.cur_align = saved_align;
        st.cur_bg = saved_bg;
        st.cur_color = saved_color;
        st.cur_href = saved_href;
        st.cur_margin = saved_margin;
        st.cur_padding = saved_padding;
        st.cur_width = saved_width;
        st.cur_border_w = saved_border_w;
        st.cur_border_c = saved_border_c;
        st.cur_clears = saved_clears;
        return;
    }

    // Block container (div, section, unknown): a flow boundary that still passes
    // inherited properties (colour, font-size, text-align) down to its children.
    flush(st);
    let saved_style = st.style;
    let saved_align = st.cur_align;
    let saved_color = st.cur_color;
    let saved_clears = st.cur_clears;
    if let Some(c) = applied.color {
        st.style.color = c;
        st.cur_color = c;
    }
    if let Some(s) = applied.font_size {
        st.style.size = s;
    }
    if let Some(al) = applied.text_align {
        st.cur_align = al.factor();
    }
    if let Some(cl) = applied.clear {
        st.cur_clears = cl;
    }
    st.stack.push(ctx);
    walk_children(parser, tag, st);
    st.stack.pop();
    flush(st);
    st.style = saved_style;
    st.cur_align = saved_align;
    st.cur_color = saved_color;
    st.cur_clears = saved_clears;
}

fn walk_children(parser: &tl::Parser, tag: &tl::HTMLTag, st: &mut State) {
    // Copy handles out first (NodeHandle is Copy) so the immutable borrow of
    // `tag` doesn't overlap the recursive mutation of `st`.
    let kids: Vec<tl::NodeHandle> = tag.children().top().iter().copied().collect();
    for handle in kids {
        walk(parser, handle, st);
    }
}

/// Append `raw` to the current block in the current inline colour/href,
/// collapsing its internal whitespace and inserting a single separating space
/// when a whitespace boundary sits between this run and the previous one. Records
/// a [`Span`] for the appended range when it's a link or its colour differs from
/// the block default. Char offsets into `buf` stay stable (it's already
/// collapsed), so spans line up with what the engine draws and hit-tests.
fn push_run(st: &mut State, raw: &str) {
    let color = st.cur_color;
    let href = st.cur_href.clone();

    let lead = raw.starts_with(|c: char| c.is_whitespace());
    let trail = raw.ends_with(|c: char| c.is_whitespace());
    let collapsed = collapse_ws(raw);

    if collapsed.is_empty() {
        // Pure whitespace: remember the boundary so runs stay separated.
        if !st.buf.is_empty() && (lead || trail) {
            st.pending_space = true;
        }
        return;
    }

    if !st.buf.is_empty() && (st.pending_space || lead) {
        st.buf.push(' ');
    }
    let start = st.buf.chars().count() as u32;
    st.buf.push_str(&collapsed);
    let end = st.buf.chars().count() as u32;
    if href.is_some() || color != st.style.color {
        st.spans.push(Span {
            start,
            end,
            color,
            href,
        });
    }
    st.pending_space = trail;
}

/// Emit the pending block as a Text block. `buf` is already collapsed/trimmed.
fn flush(st: &mut State) {
    let text = std::mem::take(&mut st.buf);
    let spans = std::mem::take(&mut st.spans);
    st.pending_space = false;
    if !text.is_empty() {
        st.blocks.push(Block::Text {
            text,
            size: st.style.size,
            color: st.style.color,
            align: st.cur_align,
            background: st.cur_bg,
            margin: st.cur_margin,
            padding: st.cur_padding,
            width: st.cur_width,
            border_w: st.cur_border_w,
            border_c: st.cur_border_c,
            spans,
            clears: st.cur_clears,
        });
    } else {
        // Drop orphan spans (shouldn't happen: spans only added alongside text).
        let _ = spans;
    }
}

/// Collapse all runs of ASCII/Unicode whitespace into single spaces and trim.
fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Decode the handful of HTML entities common in plain documents.
fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&mdash;", "—")
        .replace("&ndash;", "–")
        .replace("&hellip;", "…")
}
