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
use crate::table;

/// An inline style run within a [`Block::Text`]: char range `[start, end)` over
/// the block's `text`, a colour override, and an optional link `href`.
pub struct Span {
    pub start: u32,
    pub end: u32,
    pub color: Rgba,
    pub href: Option<String>,
    /// Resolved weight/slant for this run (folded with the block base).
    pub bold: bool,
    pub italic: bool,
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
        /// Block-base weight/slant (headings are bold). Inline runs that differ
        /// carry their own bold/italic in their [`Span`].
        bold: bool,
        italic: bool,
        align: f32,
        background: Option<Rgba>,
        margin: (f32, f32, f32, f32),
        padding: (f32, f32, f32, f32),
        width: css::WidthVal,
        /// CSS `height` (`Auto` ⇒ content-sized, the default).
        height: css::WidthVal,
        /// CSS `line-height` in px; `<= 0.0` ⇒ the font's natural line height.
        line_height: f32,
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
        /// For an inline `<svg>`: the serialized SVG source, rasterized directly
        /// at render time (no fetch). `None` for a normal `<img src=…>`, where
        /// `url` drives the fetch instead.
        inline_svg: Option<String>,
        float: css::Float,
        clears: bool,
        position: css::Position,
        /// `(top, right, bottom, left)` position offsets.
        inset: (Option<f32>, Option<f32>, Option<f32>, Option<f32>),
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
        /// CSS `position` and its `(top, right, bottom, left)` offsets.
        position: css::Position,
        inset: (Option<f32>, Option<f32>, Option<f32>, Option<f32>),
        /// Explicit box width (table cells set a percentage so columns align);
        /// `Auto` keeps the default flex sizing.
        width: css::WidthVal,
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

/// An open list context (`<ul>`/`<ol>`) used to label `<li>` children. `ordered`
/// lists carry a running `counter` (incremented per item); unordered lists draw a
/// bullet. Pushed/popped as the walker enters/leaves a list, so nested lists each
/// keep their own counter.
struct ListCtx {
    ordered: bool,
    counter: u32,
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
    /// Current inline weight/slant (differ from the block base inside
    /// `<b>`/`<strong>`/`<i>`/`<em>`/… or via inline `font-weight`/`font-style`).
    cur_bold: bool,
    cur_italic: bool,
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
    /// Current block's CSS `height` (`Auto` ⇒ content-sized).
    cur_height: css::WidthVal,
    /// Current block's CSS `line-height` in px (`0.0` ⇒ natural font height).
    cur_line_height: f32,
    cur_border_w: f32,
    cur_border_c: Rgba,
    /// Current block's `clear`: drops the block below any preceding float.
    cur_clears: bool,
    /// Open list contexts (innermost last) for `<li>` markers.
    lists: Vec<ListCtx>,
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
        cur_bold: false,
        cur_italic: false,
        cur_href: None,
        cur_bg: None,
        cur_align: 0.0,
        indent: 0.0,
        cur_margin: (0.0, 0.0, 0.0, 0.0),
        cur_padding: (0.0, 0.0, 0.0, 0.0),
        cur_width: css::WidthVal::Auto,
        cur_height: css::WidthVal::Auto,
        cur_line_height: 0.0,
        cur_border_w: 0.0,
        cur_border_c: (0.0, 0.0, 0.0, 0.0),
        cur_clears: false,
        lists: Vec::new(),
    };
    for handle in dom.children() {
        walk(parser, *handle, &mut st);
    }
    flush(&mut st);
    st.blocks
}

/// Extract `position` offsets `(top, right, bottom, left)` from a computed style.
fn applied_inset(a: &css::Applied) -> (Option<f32>, Option<f32>, Option<f32>, Option<f32>) {
    (a.inset[0], a.inset[1], a.inset[2], a.inset[3])
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
                        inline_svg: None,
                        float: applied.float.unwrap_or(css::Float::None),
                        clears: applied.clear.unwrap_or(false),
                        position: applied.position.unwrap_or(css::Position::Static),
                        inset: applied_inset(&applied),
                    });
                }
            }
            return;
        }
        "svg" => {
            // Inline SVG: serialize the whole element back to source and rasterize
            // it directly (no fetch). Cap the source size so a pathological inline
            // document can't blow out decode time/memory.
            flush(st);
            const MAX_SVG_SRC: usize = 256 * 1024;
            let src = tag.outer_html(parser);
            if !src.is_empty() && src.len() <= MAX_SVG_SRC {
                st.blocks.push(Block::Image {
                    url: String::new(),
                    inline_svg: Some(src),
                    float: applied.float.unwrap_or(css::Float::None),
                    clears: applied.clear.unwrap_or(false),
                    position: applied.position.unwrap_or(css::Position::Static),
                    inset: applied_inset(&applied),
                });
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
            let saved_bold = st.cur_bold;
            let saved_italic = st.cur_italic;
            st.cur_color = applied.color.unwrap_or(style::LINK);
            st.cur_href = href;
            if let Some(w) = applied.font_weight {
                st.cur_bold = w;
            }
            if let Some(i) = applied.font_style {
                st.cur_italic = i;
            }
            st.stack.push(ctx);
            walk_children(parser, tag, st);
            st.stack.pop();
            st.cur_color = saved_color;
            st.cur_href = saved_href;
            st.cur_bold = saved_bold;
            st.cur_italic = saved_italic;
            return;
        }
        _ => {}
    }

    // Tables: parse the whole subtree into a grid model and emit it as nested
    // flex containers with automatic column widths (browser-side layout — the
    // engine sees ordinary sized boxes). Handled wholesale so `<tr>`/`<td>`/… are
    // never walked by the normal flow.
    if name == "table" {
        flush(st);
        let mut rows = Vec::new();
        collect_table_rows(parser, tag, st, &mut rows);
        let grid = table::Grid { rows };
        if grid.rows.iter().any(|r| !r.cells.is_empty()) {
            st.blocks.push(table::build(grid));
        }
        return;
    }

    if display == css::Display::Flex {
        // Flex container: collect its children into a nested block list instead
        // of flattening them into the document flow, so the renderer can lay
        // them out along the flex axis.
        flush(st);
        let saved_style = st.style;
        let saved_align = st.cur_align;
        let saved_color = st.cur_color;
        let saved_bold = st.cur_bold;
        let saved_italic = st.cur_italic;
        let saved_href = st.cur_href.clone();
        if let Some(c) = applied.color {
            st.style.color = c;
            st.cur_color = c;
        }
        if let Some(s) = applied.font_size {
            st.style.size = s;
        }
        if let Some(w) = applied.font_weight {
            st.style.bold = w;
            st.cur_bold = w;
        }
        if let Some(i) = applied.font_style {
            st.style.italic = i;
            st.cur_italic = i;
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
        st.cur_bold = saved_bold;
        st.cur_italic = saved_italic;
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
                position: applied.position.unwrap_or(css::Position::Static),
                inset: applied_inset(&applied),
                width: css::WidthVal::Auto,
            });
        }
        return;
    }

    // Positioned block (non-flex, non-static): a discrete box that the engine
    // shifts (`relative`) or pulls out of flow and anchors (`absolute`). Made a
    // container so an `absolute` descendant resolves against it. Takes precedence
    // over float (CSS ignores float on absolutely-positioned boxes).
    let position = applied.position.unwrap_or(css::Position::Static);
    if position != css::Position::Static {
        flush(st);
        let saved_style = st.style;
        let saved_align = st.cur_align;
        let saved_color = st.cur_color;
        let saved_bold = st.cur_bold;
        let saved_italic = st.cur_italic;
        let saved_href = st.cur_href.clone();
        if let Some(c) = applied.color {
            st.style.color = c;
            st.cur_color = c;
        }
        if let Some(s) = applied.font_size {
            st.style.size = s;
        }
        if let Some(w) = applied.font_weight {
            st.style.bold = w;
            st.cur_bold = w;
        }
        if let Some(i) = applied.font_style {
            st.style.italic = i;
            st.cur_italic = i;
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
        st.cur_bold = saved_bold;
        st.cur_italic = saved_italic;
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
                float: css::Float::None,
                clears: applied.clear.unwrap_or(false),
                position,
                inset: applied_inset(&applied),
                width: css::WidthVal::Auto,
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
        let saved_bold = st.cur_bold;
        let saved_italic = st.cur_italic;
        let saved_href = st.cur_href.clone();
        if let Some(c) = applied.color {
            st.style.color = c;
            st.cur_color = c;
        }
        if let Some(s) = applied.font_size {
            st.style.size = s;
        }
        if let Some(w) = applied.font_weight {
            st.style.bold = w;
            st.cur_bold = w;
        }
        if let Some(i) = applied.font_style {
            st.style.italic = i;
            st.cur_italic = i;
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
        st.cur_bold = saved_bold;
        st.cur_italic = saved_italic;
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
                position: css::Position::Static,
                inset: (None, None, None, None),
                width: css::WidthVal::Auto,
            });
        }
        return;
    }

    if matches!(display, css::Display::Inline | css::Display::InlineBlock) {
        // Inline element: fold children into the current run, optionally applying
        // a colour override (font-size on inline boxes isn't modelled yet). UA
        // weight/slant for `b`/`strong`/`i`/`em`/… apply here, then author CSS
        // `font-weight`/`font-style` can override.
        let saved_color = st.cur_color;
        let saved_bold = st.cur_bold;
        let saved_italic = st.cur_italic;
        if let Some(c) = applied.color {
            st.cur_color = c;
        }
        st.cur_bold |= style::inline_bold(name);
        st.cur_italic |= style::inline_italic(name);
        if let Some(w) = applied.font_weight {
            st.cur_bold = w;
        }
        if let Some(i) = applied.font_style {
            st.cur_italic = i;
        }
        st.stack.push(ctx);
        walk_children(parser, tag, st);
        st.stack.pop();
        st.cur_color = saved_color;
        st.cur_bold = saved_bold;
        st.cur_italic = saved_italic;
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
        let saved_bold = st.cur_bold;
        let saved_italic = st.cur_italic;
        let saved_href = st.cur_href.clone();
        let saved_margin = st.cur_margin;
        let saved_padding = st.cur_padding;
        let saved_width = st.cur_width;
        let saved_height = st.cur_height;
        let saved_line_height = st.cur_line_height;
        let saved_border_w = st.cur_border_w;
        let saved_border_c = st.cur_border_c;
        let saved_clears = st.cur_clears;

        st.style = bs;
        st.style.size = applied.font_size.unwrap_or(bs.size);
        st.style.color = applied.color.unwrap_or(bs.color);
        st.style.bold = applied.font_weight.unwrap_or(bs.bold);
        st.style.italic = applied.font_style.unwrap_or(bs.italic);
        st.cur_color = st.style.color;
        st.cur_bold = st.style.bold;
        st.cur_italic = st.style.italic;
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
        st.cur_height = applied.height.unwrap_or(css::WidthVal::Auto);
        // `line-height` inherits, so keep the saved value when this block doesn't
        // set one (rather than resetting to natural).
        st.cur_line_height = applied.line_height.unwrap_or(st.cur_line_height);
        st.cur_border_w = applied.border_width.unwrap_or(0.0);
        // `border-color` defaults to the text colour (CSS currentColor).
        st.cur_border_c = applied.border_color.unwrap_or(st.style.color);
        st.cur_clears = applied.clear.unwrap_or(false);

        if name == "li" {
            // Marker per the innermost open list: a running number for `<ol>`, a
            // bullet for `<ul>` (or a stray `<li>` with no list ancestor). It
            // inherits the block colour ⇒ no span; pushing it before the children
            // keeps subsequent span offsets correct (push_run measures `buf`).
            let marker = match st.lists.last_mut() {
                Some(l) if l.ordered => {
                    l.counter += 1;
                    format!("{}. ", l.counter)
                }
                _ => "• ".to_string(),
            };
            push_run(st, &marker);
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
        st.cur_bold = saved_bold;
        st.cur_italic = saved_italic;
        st.cur_href = saved_href;
        st.cur_margin = saved_margin;
        st.cur_padding = saved_padding;
        st.cur_width = saved_width;
        st.cur_height = saved_height;
        st.cur_line_height = saved_line_height;
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
    let saved_bold = st.cur_bold;
    let saved_italic = st.cur_italic;
    let saved_clears = st.cur_clears;
    let saved_line_height = st.cur_line_height;
    if let Some(c) = applied.color {
        st.style.color = c;
        st.cur_color = c;
    }
    if let Some(s) = applied.font_size {
        st.style.size = s;
    }
    // `line-height` inherits down to descendant text blocks.
    if let Some(lh) = applied.line_height {
        st.cur_line_height = lh;
    }
    if let Some(w) = applied.font_weight {
        st.style.bold = w;
        st.cur_bold = w;
    }
    if let Some(i) = applied.font_style {
        st.style.italic = i;
        st.cur_italic = i;
    }
    if let Some(al) = applied.text_align {
        st.cur_align = al.factor();
    }
    if let Some(cl) = applied.clear {
        st.cur_clears = cl;
    }
    // A list container starts a fresh marker context for its `<li>` children;
    // nesting stacks so inner lists restart their counters.
    let is_list = name == "ul" || name == "ol";
    if is_list {
        st.lists.push(ListCtx {
            ordered: name == "ol",
            counter: 0,
        });
    }
    st.stack.push(ctx);
    walk_children(parser, tag, st);
    st.stack.pop();
    if is_list {
        st.lists.pop();
    }
    flush(st);
    st.style = saved_style;
    st.cur_align = saved_align;
    st.cur_color = saved_color;
    st.cur_bold = saved_bold;
    st.cur_italic = saved_italic;
    st.cur_clears = saved_clears;
    st.cur_line_height = saved_line_height;
}

/// Parse a `colspan`/`rowspan`-style positive integer attribute.
fn attr_usize(tag: &tl::HTMLTag, key: &str) -> Option<usize> {
    tag.attributes()
        .get(key)
        .flatten()
        .and_then(|b| b.as_utf8_str().trim().parse::<usize>().ok())
        .filter(|&n| n >= 1)
}

/// Collect a table's rows, descending through `<thead>`/`<tbody>`/`<tfoot>`
/// grouping wrappers so their `<tr>`s land in document order.
fn collect_table_rows(
    parser: &tl::Parser,
    tag: &tl::HTMLTag,
    st: &mut State,
    rows: &mut Vec<table::GridRow>,
) {
    let kids: Vec<tl::NodeHandle> = tag.children().top().iter().copied().collect();
    for h in kids {
        let Some(tl::Node::Tag(child)) = h.get(parser) else {
            continue;
        };
        let name = child.name().as_utf8_str().to_ascii_lowercase();
        match name.as_str() {
            "tr" => rows.push(parse_table_row(parser, child, st)),
            "thead" | "tbody" | "tfoot" => collect_table_rows(parser, child, st, rows),
            _ => {}
        }
    }
}

/// Parse one `<tr>` into a row of cells (`<td>`/`<th>`).
fn parse_table_row(parser: &tl::Parser, tag: &tl::HTMLTag, st: &mut State) -> table::GridRow {
    let mut cells = Vec::new();
    let kids: Vec<tl::NodeHandle> = tag.children().top().iter().copied().collect();
    for h in kids {
        let Some(tl::Node::Tag(child)) = h.get(parser) else {
            continue;
        };
        let name = child.name().as_utf8_str().to_ascii_lowercase();
        match name.as_str() {
            "td" => cells.push(parse_table_cell(parser, child, st, false)),
            "th" => cells.push(parse_table_cell(parser, child, st, true)),
            _ => {}
        }
    }
    table::GridRow { cells }
}

/// Parse a single cell: walk its children into a fresh block list under a fresh
/// inline style (headers are bold + centred), then measure its min/max content
/// width for the column algorithm.
fn parse_table_cell(
    parser: &tl::Parser,
    tag: &tl::HTMLTag,
    st: &mut State,
    header: bool,
) -> table::Cell {
    let colspan = attr_usize(tag, "colspan").unwrap_or(1);
    let rowspan = attr_usize(tag, "rowspan").unwrap_or(1);

    // Divert content into a fresh list with a fresh inline style, mirroring the
    // flex-container save/restore so the cell is fully isolated.
    let saved_blocks = std::mem::take(&mut st.blocks);
    let saved_style = st.style;
    let saved_align = st.cur_align;
    let saved_color = st.cur_color;
    let saved_bold = st.cur_bold;
    let saved_italic = st.cur_italic;
    let saved_href = st.cur_href.clone();
    let saved_bg = st.cur_bg;
    let saved_indent = st.indent;
    let saved_margin = st.cur_margin;
    let saved_padding = st.cur_padding;
    let saved_width = st.cur_width;
    let saved_border_w = st.cur_border_w;
    let saved_border_c = st.cur_border_c;
    let saved_clears = st.cur_clears;

    st.style = TextStyle::body();
    st.style.bold = header;
    st.cur_color = st.style.color;
    st.cur_bold = header;
    st.cur_italic = false;
    st.cur_href = None;
    st.cur_bg = None;
    st.cur_align = if header { 0.5 } else { 0.0 };
    st.indent = 0.0;
    st.cur_margin = (0.0, 0.0, 0.0, 0.0);
    st.cur_padding = (0.0, 0.0, 0.0, 0.0);
    st.cur_width = css::WidthVal::Auto;
    st.cur_border_w = 0.0;
    st.cur_border_c = (0.0, 0.0, 0.0, 0.0);
    st.cur_clears = false;

    let ctx = elem_ctx(if header { "th" } else { "td" }, tag);
    st.stack.push(ctx);
    walk_children(parser, tag, st);
    st.stack.pop();
    flush(st);
    let blocks = std::mem::replace(&mut st.blocks, saved_blocks);

    st.style = saved_style;
    st.cur_align = saved_align;
    st.cur_color = saved_color;
    st.cur_bold = saved_bold;
    st.cur_italic = saved_italic;
    st.cur_href = saved_href;
    st.cur_bg = saved_bg;
    st.indent = saved_indent;
    st.cur_margin = saved_margin;
    st.cur_padding = saved_padding;
    st.cur_width = saved_width;
    st.cur_border_w = saved_border_w;
    st.cur_border_c = saved_border_c;
    st.cur_clears = saved_clears;

    let (min_w, max_w) = table::measure_blocks(&blocks);
    table::Cell {
        blocks,
        colspan,
        rowspan,
        header,
        min_w: min_w + table::CELL_PAD_X,
        max_w: max_w + table::CELL_PAD_X,
    }
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
    let bold = st.cur_bold;
    let italic = st.cur_italic;

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
    if href.is_some()
        || color != st.style.color
        || bold != st.style.bold
        || italic != st.style.italic
    {
        st.spans.push(Span {
            start,
            end,
            color,
            href,
            bold,
            italic,
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
            bold: st.style.bold,
            italic: st.style.italic,
            align: st.cur_align,
            background: st.cur_bg,
            margin: st.cur_margin,
            padding: st.cur_padding,
            width: st.cur_width,
            height: st.cur_height,
            line_height: st.cur_line_height,
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

/// Decode HTML character references in `s`: numeric `&#NN;` / `&#xHH;` (via
/// `char::from_u32`) and a table of the common named entities. An unrecognised or
/// unterminated reference is left verbatim (so stray `&`s survive). These map to
/// real glyphs now that the atlas covers Latin-1 + curated punctuation.
fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'&' {
            // Copy the next whole UTF-8 char (find the following char boundary).
            let mut j = i + 1;
            while j < bytes.len() && (bytes[j] & 0xC0) == 0x80 {
                j += 1;
            }
            out.push_str(&s[i..j]);
            i = j;
            continue;
        }
        // Look for the terminating ';' within a bounded window (longest real
        // entity name is well under this; avoids scanning a whole document on a
        // bare '&').
        let end = s[i..]
            .char_indices()
            .take(32)
            .find(|&(_, c)| c == ';')
            .map(|(o, _)| i + o);
        if let Some(semi) = end {
            let body = &s[i + 1..semi]; // between '&' and ';'
            if let Some(ch) = decode_reference(body) {
                out.push(ch);
                i = semi + 1;
                continue;
            }
        }
        // Not a recognised reference: emit the '&' literally and move on.
        out.push('&');
        i += 1;
    }
    out
}

/// Decode the inside of a reference (`amp`, `#8217`, `#x2014`) to a char, or
/// `None` if unknown/invalid.
fn decode_reference(body: &str) -> Option<char> {
    if let Some(num) = body.strip_prefix('#') {
        let code = if let Some(hex) = num.strip_prefix(['x', 'X']) {
            u32::from_str_radix(hex, 16).ok()?
        } else {
            num.parse::<u32>().ok()?
        };
        return char::from_u32(code);
    }
    let ch = match body {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{00A0}',
        "mdash" => '—',
        "ndash" => '–',
        "rsquo" => '’',
        "lsquo" => '‘',
        "ldquo" => '“',
        "rdquo" => '”',
        "sbquo" => '‚',
        "bdquo" => '„',
        "hellip" => '…',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "bull" => '•',
        "middot" => '·',
        "deg" => '°',
        "plusmn" => '±',
        "times" => '×',
        "divide" => '÷',
        "frac12" => '½',
        "frac14" => '¼',
        "frac34" => '¾',
        "sup2" => '²',
        "sup3" => '³',
        "laquo" => '«',
        "raquo" => '»',
        "cent" => '¢',
        "pound" => '£',
        "euro" => '€',
        "yen" => '¥',
        "sect" => '§',
        "para" => '¶',
        "dagger" => '†',
        "Dagger" => '‡',
        "prime" => '′',
        "Prime" => '″',
        "micro" => 'µ',
        "iexcl" => '¡',
        "iquest" => '¿',
        "shy" => '\u{00AD}',
        "ensp" => '\u{2002}',
        "emsp" => '\u{2003}',
        "thinsp" => '\u{2009}',
        "larr" => '←',
        "rarr" => '→',
        "uarr" => '↑',
        "darr" => '↓',
        "harr" => '↔',
        // Frequent accented Latin-1 names.
        "agrave" => 'à',
        "aacute" => 'á',
        "acirc" => 'â',
        "atilde" => 'ã',
        "auml" => 'ä',
        "aring" => 'å',
        "aelig" => 'æ',
        "ccedil" => 'ç',
        "egrave" => 'è',
        "eacute" => 'é',
        "ecirc" => 'ê',
        "euml" => 'ë',
        "igrave" => 'ì',
        "iacute" => 'í',
        "icirc" => 'î',
        "iuml" => 'ï',
        "ntilde" => 'ñ',
        "ograve" => 'ò',
        "oacute" => 'ó',
        "ocirc" => 'ô',
        "otilde" => 'õ',
        "ouml" => 'ö',
        "oslash" => 'ø',
        "ugrave" => 'ù',
        "uacute" => 'ú',
        "ucirc" => 'û',
        "uuml" => 'ü',
        "yacute" => 'ý',
        "yuml" => 'ÿ',
        "szlig" => 'ß',
        "Agrave" => 'À',
        "Aacute" => 'Á',
        "Acirc" => 'Â',
        "Atilde" => 'Ã',
        "Auml" => 'Ä',
        "Aring" => 'Å',
        "AElig" => 'Æ',
        "Ccedil" => 'Ç',
        "Egrave" => 'È',
        "Eacute" => 'É',
        "Ecirc" => 'Ê',
        "Euml" => 'Ë',
        "Ntilde" => 'Ñ',
        "Ograve" => 'Ò',
        "Oacute" => 'Ó',
        "Ocirc" => 'Ô',
        "Otilde" => 'Õ',
        "Ouml" => 'Ö',
        "Oslash" => 'Ø',
        "Ugrave" => 'Ù',
        "Uacute" => 'Ú',
        "Ucirc" => 'Û',
        "Uuml" => 'Ü',
        "Yacute" => 'Ý',
        _ => return None,
    };
    Some(ch)
}
