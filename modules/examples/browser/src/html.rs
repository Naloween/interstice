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
        /// This is a reified inline-level box (an `inline-block` `<button>` /
        /// form control). Consecutive inline-level siblings are wrapped into one
        /// horizontal row by [`group_inline_blocks`] — so e.g. the portal's search
        /// button lays out beside the search input instead of below it.
        inline_level: bool,
        /// The DOM element this block's text belongs to (innermost block-level
        /// ancestor), for routing clicks back to JS event listeners. `None` for
        /// text with no owning element (rare) or synthetic blocks.
        node: Option<crate::dom::NodeId>,
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
        /// `(top, right, bottom, left)` position offsets in px.
        inset: (Option<f32>, Option<f32>, Option<f32>, Option<f32>),
        /// The same four sides as fractions of the containing block, for `%`
        /// offsets; resolved by the engine at layout time.
        inset_pct: (Option<f32>, Option<f32>, Option<f32>, Option<f32>),
        /// Explicit display size in px from the `<img>` `width`/`height` attributes
        /// or CSS `width`/`height`. `None` ⇒ use the decoded intrinsic size. Set so
        /// e.g. the Wikipedia globe (`width="200" height="183"`, but a ~2× native
        /// PNG) renders at its authored size instead of swamping its neighbours.
        width: Option<f32>,
        height: Option<f32>,
        /// CSS-sprite crop, as `(sx, sy, sw, sh)` in the atlas's own CSS-px
        /// coordinate space (the sub-rectangle of the sheet this icon shows). The
        /// UV window is resolved against the decoded atlas size once it loads (see
        /// [`crate::images::place_image`]). `None` for a normal whole-image draw.
        sprite_crop: Option<(f32, f32, f32, f32)>,
        /// The source `<img>`/`<svg>` DOM node, for click→event routing.
        node: Option<crate::dom::NodeId>,
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
        /// The same four sides as fractions of the containing block, for `%`
        /// offsets; resolved by the engine at layout time.
        inset_pct: (Option<f32>, Option<f32>, Option<f32>, Option<f32>),
        /// Explicit box width (table cells set a percentage so columns align);
        /// `Auto` keeps the default flex sizing.
        width: css::WidthVal,
        /// Explicit box height (`Auto` ⇒ content-sized). A fixed height matters
        /// for positioned boxes whose children are all `absolute`: without it the
        /// box collapses to zero and percentage `top`/`bottom` insets resolve
        /// against nothing.
        height: css::WidthVal,
        /// The source DOM element this container was built from, for click→event
        /// routing. `None` for synthetic containers (table structure, float rows).
        node: Option<crate::dom::NodeId>,
        /// `margin: 0 auto` on a definite width ⇒ center this box on its
        /// container's cross axis (see [`wants_center`]).
        center: bool,
        /// CSS `max-height` in px; `None` ⇒ uncapped. Drives the engine's height
        /// clamp so an accordion's `max-height:0` closed state collapses and
        /// clips instead of dumping its full content (e.g. the language list).
        max_height: Option<f32>,
        /// This box is an `inline-block` (a reified inline-level box). Consecutive
        /// inline-level siblings are wrapped into one horizontal row by
        /// [`group_inline_blocks`] so e.g. the footer's `.other-project-icon` +
        /// `.other-project-text` sit side by side. `false` for ordinary block/flex
        /// containers (the common case).
        inline_level: bool,
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
    /// Current block's inherited CSS `line-height` spec (unitless multiplier or a
    /// fixed px). `None` ⇒ the font's natural height. Resolved to px in [`flush`]
    /// against the block's own font-size, so a unitless value scales per element.
    cur_line_height: Option<css::LineHeight>,
    cur_border_w: f32,
    cur_border_c: Rgba,
    /// Current block's `clear`: drops the block below any preceding float.
    cur_clears: bool,
    /// The resolved `<html>`/`<body>` `background-color`, painted as the page
    /// canvas. `None` ⇒ keep the browser's default white canvas.
    page_bg: Option<Rgba>,
    /// Open list contexts (innermost last) for `<li>` markers.
    lists: Vec<ListCtx>,
    /// The innermost block-level element currently being walked. [`flush`] stamps
    /// each emitted [`Block::Text`] with it so a click on that text can be routed
    /// back to the element's JS event listeners. Saved/restored around every
    /// block-level element (text or container).
    block_owner: Option<crate::dom::NodeId>,
}

/// Parse the current document (the stored [`crate::dom`] arena) against `sheet`
/// and return its blocks plus the resolved page (`<html>`/`<body>`) background
/// colour, if any. An empty arena yields an empty list.
pub fn parse_html(sheet: &css::Stylesheet) -> (Vec<Block>, Option<Rgba>) {
    crate::dom::with(|dom| {
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
            cur_line_height: None,
            cur_border_w: 0.0,
            cur_border_c: (0.0, 0.0, 0.0, 0.0),
            cur_clears: false,
            page_bg: None,
            lists: Vec::new(),
            block_owner: None,
        };
        for &id in dom.roots() {
            walk(dom, id, &mut st);
        }
        flush(&mut st);
        (st.blocks, st.page_bg)
    })
}

/// Extract `position` offsets `(top, right, bottom, left)` in px from a style.
fn applied_inset(a: &css::Applied) -> (Option<f32>, Option<f32>, Option<f32>, Option<f32>) {
    (a.inset[0], a.inset[1], a.inset[2], a.inset[3])
}

/// Extract the percentage `position` offsets `(top, right, bottom, left)` (as
/// fractions) from a style, for offsets authored as `%`.
fn applied_inset_pct(a: &css::Applied) -> (Option<f32>, Option<f32>, Option<f32>, Option<f32>) {
    (a.inset_pct[0], a.inset_pct[1], a.inset_pct[2], a.inset_pct[3])
}

/// Resolve an `<img>`'s explicit display width/height in px: CSS `width`/`height`
/// (as a fixed px length) wins over the HTML `width`/`height` presentational
/// attributes; either may be absent (`None` ⇒ derive from the decoded intrinsic
/// size). A bare attribute number is CSS pixels.
fn img_dimensions(node: &crate::dom::Node, applied: &css::Applied) -> (Option<f32>, Option<f32>) {
    let from_attr = |name: &str| {
        node.attr(name)
            .map(str::trim)
            .and_then(|s| s.trim_end_matches("px").trim().parse::<f32>().ok())
            .filter(|v| *v > 0.0)
    };
    let from_css = |wv: Option<css::WidthVal>| match wv {
        Some(css::WidthVal::Px(px)) if px > 0.0 => Some(px),
        _ => None,
    };
    (
        from_css(applied.width).or_else(|| from_attr("width")),
        from_css(applied.height).or_else(|| from_attr("height")),
    )
}

/// Whether a block wants the `margin: 0 auto` centering idiom: `margin-left` and
/// `margin-right` both `auto` on a *definite* width. The engine centers such a
/// box on its container's cross axis (`UiElement::center_cross`) — no wrapper box,
/// so an overflowing positioned child (e.g. the Wikipedia globe, taller than its
/// box) keeps its ancestors' clip instead of being cut by a synthetic wrapper.
fn wants_center(applied: &css::Applied) -> bool {
    applied.margin_auto[1]
        && applied.margin_auto[3]
        && matches!(
            applied.width,
            Some(css::WidthVal::Px(_)) | Some(css::WidthVal::Pct(_))
        )
}

/// The used `width` for a box that reifies a container. An explicit CSS width
/// wins; otherwise an `inline-block` box shrinks to its content (CSS shrink-to-fit)
/// rather than stretching to fill its container, while a plain block fills it.
fn box_width(applied: &css::Applied, display: css::Display) -> css::WidthVal {
    match applied.width {
        Some(w) => w,
        None if display == css::Display::InlineBlock => css::WidthVal::Fit,
        None => css::WidthVal::Auto,
    }
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

/// Recursively compose a raw flow into laid-out blocks: pair consecutive
/// inline-level boxes into horizontal rows, then pair floats with the content
/// beside them — at this level *and* inside every nested container. The walker
/// only ever builds one level of flow at a time (a reified `inline-block`/float/
/// flex box holds its children as a raw list), so the composition passes are
/// pushed down here rather than run once on the top level. Called once by the
/// renderer on the document flow.
pub fn layout_blocks(blocks: Vec<Block>) -> Vec<Block> {
    // Bottom-up: compose each child's own subtree first, then this level.
    let blocks: Vec<Block> = blocks.into_iter().map(recurse_layout).collect();
    let blocks = group_inline_blocks(blocks);
    group_floats(blocks)
}

/// Compose the flow nested inside one block (its container children / float-row
/// parts), leaving leaf blocks untouched.
fn recurse_layout(mut b: Block) -> Block {
    match &mut b {
        Block::Container { children, .. } => {
            let kids = std::mem::take(children);
            *children = layout_blocks(kids);
        }
        Block::FloatRow { float_box, flow, .. } => {
            let fb = std::mem::replace(float_box.as_mut(), Block::Space { height: 0.0 });
            **float_box = recurse_layout(fb);
            let f = std::mem::take(flow);
            *flow = layout_blocks(f);
        }
        _ => {}
    }
    b
}

/// Whether a block is a reified `inline-block` box (an inline-level box that
/// wants to sit beside its inline-level siblings on one line).
fn is_inline_level(b: &Block) -> bool {
    matches!(
        b,
        Block::Container {
            inline_level: true,
            ..
        } | Block::Text {
            inline_level: true,
            ..
        }
    )
}

/// Wrap a run of `inline_level` children into one horizontal row so they lay out
/// side by side (icon + text in the footer's sister-project cards, the search
/// field + language picker, …). Cross-aligned to centre to mimic CSS
/// `vertical-align:middle`. A lone inline-block is left as-is (it just sits as a
/// block, which is fine — nothing to line it up with).
fn group_inline_blocks(blocks: Vec<Block>) -> Vec<Block> {
    let mut out = Vec::new();
    let mut run: Vec<Block> = Vec::new();
    for b in blocks {
        if is_inline_level(&b) {
            run.push(b);
        } else {
            flush_inline_run(&mut run, &mut out);
            out.push(b);
        }
    }
    flush_inline_run(&mut run, &mut out);
    out
}

fn flush_inline_run(run: &mut Vec<Block>, out: &mut Vec<Block>) {
    if run.len() >= 2 {
        out.push(inline_row(std::mem::take(run)));
    } else {
        out.append(run);
    }
}

/// A synthetic horizontal row holding a run of inline-level boxes.
fn inline_row(children: Vec<Block>) -> Block {
    Block::Container {
        direction: css::FlexDirection::Row,
        justify: css::Justify::Start,
        align: css::Align::Center,
        gap: 6.0,
        margin: (0.0, 0.0, 0.0, 0.0),
        padding: (0.0, 0.0, 0.0, 0.0),
        background: None,
        children,
        float: css::Float::None,
        clears: false,
        position: css::Position::Static,
        inset: (None, None, None, None),
        inset_pct: (None, None, None, None),
        width: css::WidthVal::Auto,
        height: css::WidthVal::Auto,
        node: None,
        center: false,
        max_height: None,
        inline_level: false,
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
        // A run of ≥2 directly-consecutive floats sharing this one's side *and*
        // width is a multi-column band (e.g. the Wikipedia footer's twelve
        // `width:33%` sister-project cards): lay them out as a wrapping grid
        // rather than stacking each in its own column.
        if same_grid_float(q.front(), side, block_box_width(&b)) {
            let w = block_box_width(&b);
            let mut run = vec![b];
            while same_grid_float(q.front(), side, w) {
                run.push(q.pop_front().unwrap());
            }
            out.push(build_float_grid(run));
            continue;
        }
        // Otherwise this is a single float paired with the in-flow content beside
        // it — which may itself begin with a sibling-float grid (e.g. the footer's
        // 35% app-download column sitting beside the sister-project grid).
        let mut flow = Vec::new();
        loop {
            if let Some(grid) = take_float_grid(&mut q) {
                flow.push(grid);
                continue;
            }
            match q.front() {
                Some(next)
                    if !block_clears(next) && block_float(next) == css::Float::None =>
                {
                    flow.push(q.pop_front().unwrap());
                }
                _ => break,
            }
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

/// The box width of a float, for grid-run matching (`None` for non-box floats).
fn block_box_width(b: &Block) -> Option<css::WidthVal> {
    match b {
        Block::Container { width, .. } => Some(*width),
        _ => None,
    }
}

/// Whether `blk` is a float on `side` with the same box width as `want` — i.e. a
/// sibling column in a multi-column float band. A definite width is required
/// (`None`/`Auto` floats are intrinsic-width, not grid columns).
fn same_grid_float(blk: Option<&Block>, side: css::Float, want: Option<css::WidthVal>) -> bool {
    let Some(blk) = blk else { return false };
    if block_clears(blk) || block_float(blk) != side {
        return false;
    }
    matches!(want, Some(css::WidthVal::Px(_)) | Some(css::WidthVal::Pct(_)))
        && block_box_width(blk) == want
}

/// If the front of `q` begins a run of ≥2 same-side, same-width sibling floats,
/// consume the whole run and return it as a grid; otherwise leave `q` untouched.
fn take_float_grid(q: &mut std::collections::VecDeque<Block>) -> Option<Block> {
    let first = q.front()?;
    let side = block_float(first);
    if side == css::Float::None {
        return None;
    }
    let w = block_box_width(first);
    // Need at least two matching floats to form a band.
    if !same_grid_float(q.get(1), side, w) {
        return None;
    }
    let mut run = Vec::new();
    while same_grid_float(q.front(), side, w) {
        run.push(q.pop_front().unwrap());
    }
    Some(build_float_grid(run))
}

/// Build a wrapping grid from a run of equal-width sibling floats: chunk them into
/// rows of `round(1/width)` columns (a percentage width) — or a single row for a
/// pixel width — and stack the rows. The engine has no flex-wrap, so the wrap is
/// materialized here as a column of row containers.
fn build_float_grid(items: Vec<Block>) -> Block {
    let cols = match items.first().and_then(block_box_width) {
        Some(css::WidthVal::Pct(f)) if f > 0.02 => (1.0 / f).round().max(1.0) as usize,
        _ => items.len().max(1),
    };
    let plain = |direction, children: Vec<Block>| Block::Container {
        direction,
        justify: css::Justify::Start,
        align: css::Align::Start,
        gap: 8.0,
        margin: (0.0, 0.0, 0.0, 0.0),
        padding: (0.0, 0.0, 0.0, 0.0),
        background: None,
        children,
        float: css::Float::None,
        clears: false,
        position: css::Position::Static,
        inset: (None, None, None, None),
        inset_pct: (None, None, None, None),
        width: css::WidthVal::Auto,
        height: css::WidthVal::Auto,
        node: None,
        center: false,
        max_height: None,
        inline_level: false,
    };
    let mut items = items;
    let mut rows = Vec::new();
    while !items.is_empty() {
        let take = cols.min(items.len());
        let row: Vec<Block> = items.drain(0..take).collect();
        rows.push(plain(css::FlexDirection::Row, row));
    }
    plain(css::FlexDirection::Column, rows)
}

/// Scan the current document arena for stylesheet sources: external
/// `<link rel=stylesheet href>` targets (resolved later) and inline `<style>`
/// text, in source order.
pub fn collect_stylesheets() -> (Vec<String>, Vec<String>) {
    crate::dom::with(|dom| {
        let mut links = Vec::new();
        let mut inline = Vec::new();
        for &id in dom.roots() {
            collect_walk(dom, id, &mut links, &mut inline);
        }
        (links, inline)
    })
}

fn collect_walk(
    dom: &crate::dom::Dom,
    id: crate::dom::NodeId,
    links: &mut Vec<String>,
    inline: &mut Vec<String>,
) {
    let node = dom.node(id);
    let Some(name) = node.tag() else {
        return;
    };
    match name {
        "link" => {
            let rel = node.attr("rel").unwrap_or_default().to_ascii_lowercase();
            if rel.split_whitespace().any(|t| t == "stylesheet") {
                if let Some(href) = node.attr("href") {
                    let h = href.trim().to_string();
                    if !h.is_empty() {
                        links.push(h);
                    }
                }
            }
        }
        "style" => {
            let css_text = dom.inner_text(id);
            if !css_text.trim().is_empty() {
                inline.push(css_text);
            }
        }
        _ => {}
    }
    let kids = node.children.clone();
    for k in kids {
        collect_walk(dom, k, links, inline);
    }
}

fn walk(dom: &crate::dom::Dom, id: crate::dom::NodeId, st: &mut State) {
    match &dom.node(id).kind {
        crate::dom::NodeKind::Text(t) => {
            push_run(st, &decode_entities(t));
        }
        crate::dom::NodeKind::Comment(_) => {}
        crate::dom::NodeKind::Element { tag, .. } => {
            let name = tag.clone();
            // Generated content (`::before`/`::after`): only pay the per-element
            // pseudo query when the sheet actually declares such rules and the
            // element isn't skipped. `st.stack` here excludes this element (matching
            // how `applied` is called), so the pseudo matches the element itself.
            if st.sheet.has_pseudo() && !style::is_skipped(&name) {
                let ctx = elem_ctx(dom, id, &name);
                let base_font = st.style.size;
                let before = st.sheet.pseudo(&ctx, &st.stack, css::PseudoEl::Before, base_font);
                let after = st.sheet.pseudo(&ctx, &st.stack, css::PseudoEl::After, base_font);
                inject_pseudo(st, before.as_ref());
                tag_node(dom, id, &name, st);
                inject_pseudo(st, after.as_ref());
            } else {
                tag_node(dom, id, &name, st);
            }
        }
    }
}

/// Emit a `::before`/`::after` generated-content string into the current inline
/// run, honouring an explicit pseudo `color` (else inheriting the current one).
/// No-op when there's no content (already filtered by [`css::Stylesheet::pseudo`]).
fn inject_pseudo(st: &mut State, applied: Option<&css::Applied>) {
    let Some(applied) = applied else { return };
    let Some(content) = applied.content.as_deref() else { return };
    if content.is_empty() {
        return;
    }
    let saved_color = st.cur_color;
    if let Some(c) = applied.color {
        st.cur_color = c;
    }
    push_run(st, content);
    st.cur_color = saved_color;
}

/// Build the selector-matching context (tag/id/classes) for an element.
fn elem_ctx(dom: &crate::dom::Dom, nid: crate::dom::NodeId, name: &str) -> css::ElemCtx {
    let node = dom.node(nid);
    let id = node
        .attr("id")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let classes = node
        .attr("class")
        .map(|s| s.split_whitespace().map(|c| c.to_string()).collect())
        .unwrap_or_default();
    let attrs = match &node.kind {
        crate::dom::NodeKind::Element { attrs, .. } => attrs.clone(),
        _ => Vec::new(),
    };

    // Position among element siblings (1-based), plus the same-tag counts, for
    // the structural pseudo-classes. `<html>`/orphan nodes report unknown (0).
    let (mut index, mut count, mut type_index, mut type_count) = (0usize, 0usize, 0usize, 0usize);
    let siblings: &[crate::dom::NodeId] = match node.parent {
        Some(p) => &dom.node(p).children,
        None => &[],
    };
    for &sib in siblings {
        // Only element siblings count for :nth-child / :first-child etc.
        let Some(stag) = dom.node(sib).tag() else {
            continue;
        };
        count += 1;
        let same_type = stag == name;
        if same_type {
            type_count += 1;
        }
        if sib == nid {
            index = count;
            type_index = type_count;
        }
    }

    css::ElemCtx {
        tag: name.to_string(),
        id,
        classes,
        attrs,
        index,
        count,
        type_index,
        type_count,
        is_root: name == "html",
    }
}

/// Pick the best URL for an `<img>`: a usable `src`, else the first `srcset`
/// candidate, else a lazy-loading `data-src`/`data-srcset`. Returns `None` when
/// there's nothing fetchable (e.g. a `src=""` placeholder with no fallback).
fn img_url(node: &crate::dom::Node) -> Option<String> {
    // A non-empty, non-data-URI `src` is the common case.
    if let Some(src) = node.attr("src") {
        let s = src.trim();
        if !s.is_empty() && !s.starts_with("data:image/gif") {
            return Some(s.to_string());
        }
    }
    // `srcset` / lazy-load fallbacks (Wikipedia thumbs, responsive images).
    for attr in ["srcset", "data-srcset", "data-src"] {
        if let Some(val) = node.attr(attr) {
            if let Some(u) = first_srcset_url(val) {
                return Some(u);
            }
        }
    }
    None
}

/// Extract the first candidate URL from a `srcset` (comma-separated `url [wd]`),
/// or treat the whole string as a single URL (`data-src`).
fn first_srcset_url(srcset: &str) -> Option<String> {
    let first = srcset.split(',').next()?.trim();
    let url = first.split_whitespace().next()?.trim();
    (!url.is_empty()).then(|| url.to_string())
}

/// The UA default `display` for a tag (author CSS may override it).
fn ua_display(name: &str) -> css::Display {
    if name == "a" || style::is_inline(name) {
        css::Display::Inline
    } else {
        css::Display::Block
    }
}

fn tag_node(dom: &crate::dom::Dom, id: crate::dom::NodeId, name: &str, st: &mut State) {
    if style::is_skipped(name) {
        return;
    }

    let ctx = elem_ctx(dom, id, name);
    let base_font = st.style.size;
    let mut applied = st.sheet.applied(&ctx, &st.stack, base_font);
    // Inline `style="…"` wins over author rules.
    if let Some(style_attr) = dom.node(id).attr("style") {
        let inline = st.sheet.inline(style_attr, base_font);
        applied.overlay(&inline);
    }

    // The page canvas colour comes from `<html>`/`<body>` — capture it so the
    // viewport paints it behind the whole document (a later `<body>` wins).
    if (name == "body" || name == "html") && applied.background.is_some() {
        st.page_bg = applied.background;
    }

    let display = applied.display.unwrap_or_else(|| ua_display(name));
    // UA rule `dialog:not([open]){display:none}`: a closed `<dialog>` (and its
    // whole subtree) is hidden. Author CSS setting an explicit `display` still
    // wins (respected via `applied.display`). Without this, fundraising/consent
    // dialogs baked into the static markup render their inner art as stray glyphs.
    let display = if name == "dialog"
        && applied.display.is_none()
        && dom.node(id).attr("open").is_none()
    {
        css::Display::None
    } else {
        display
    };
    if display == css::Display::None {
        return; // skip the whole subtree
    }

    // A sized element backed by a single image is almost always an icon (e.g. the
    // Wikipedia footer's sister-project logos). We can't paint true CSS
    // backgrounds, but for a definitely-sized icon we can surface the image inline.
    // Two shapes: a standalone `background-image:url(...)` draws the whole image,
    // and a CSS sprite (`.sprite` sets `linear-gradient(...),url(sheet)`, the
    // per-icon rule sets `background-position`+size) crops one cell out of the
    // shared sheet. The size cap keeps this to icon-scale art.
    if let (Some(w), Some(h)) = (
        match applied.width {
            Some(css::WidthVal::Px(w)) => Some(w),
            _ => None,
        },
        match applied.height {
            Some(css::WidthVal::Px(h)) => Some(h),
            _ => None,
        },
    ) {
        // Prefer the sprite sheet (the `.sprite` atlas URL) when present; the crop
        // window is `background-position` (a negative offset shifts the sheet, so
        // the shown rect starts at `-position`), sized to the icon box. A
        // `background-size` rescales the sheet's coordinate space (e.g. the
        // wordmark's `background-size:100%`), which our intrinsic-size UV mapping
        // can't honor — skip the sprite there rather than crop the wrong cell.
        let sprite = applied
            .sprite_atlas
            .clone()
            .filter(|_| !applied.background_size_set)
            .map(|atlas| {
                let px = applied.background_position[0].unwrap_or(0.0);
                let py = applied.background_position[1].unwrap_or(0.0);
                (atlas, Some((-px, -py, w, h)))
            });
        let img = sprite.or_else(|| applied.background_image.clone().map(|u| (u, None)));
        if let Some((url, sprite_crop)) = img {
            if w > 0.0 && h > 0.0 && w <= 400.0 && h <= 400.0 {
                flush(st);
                st.blocks.push(Block::Image {
                    url,
                    inline_svg: None,
                    float: applied.float.unwrap_or(css::Float::None),
                    clears: applied.clear.unwrap_or(false),
                    position: applied.position.unwrap_or(css::Position::Static),
                    inset: applied_inset(&applied),
                    inset_pct: applied_inset_pct(&applied),
                    width: Some(w),
                    height: Some(h),
                    sprite_crop,
                    node: Some(id),
                });
                // Icons are empty elements; nothing more to descend into.
                return;
            }
        }
    }

    // Replaced / void elements (handled regardless of display, aside from none).
    match name {
        "img" => {
            flush(st);
            if let Some(url) = img_url(dom.node(id)) {
                let (w, h) = img_dimensions(dom.node(id), &applied);
                st.blocks.push(Block::Image {
                    url,
                    inline_svg: None,
                    float: applied.float.unwrap_or(css::Float::None),
                    clears: applied.clear.unwrap_or(false),
                    position: applied.position.unwrap_or(css::Position::Static),
                    inset: applied_inset(&applied),
                    inset_pct: applied_inset_pct(&applied),
                    width: w,
                    height: h,
                    sprite_crop: None,
                    node: Some(id),
                });
            }
            return;
        }
        "svg" => {
            // Inline SVG: serialize the whole element back to source and rasterize
            // it directly (no fetch). Cap the source size so a pathological inline
            // document can't blow out decode time/memory.
            flush(st);
            const MAX_SVG_SRC: usize = 256 * 1024;
            let src = dom.serialize(id);
            if !src.is_empty() && src.len() <= MAX_SVG_SRC {
                let (w, h) = img_dimensions(dom.node(id), &applied);
                st.blocks.push(Block::Image {
                    url: String::new(),
                    inline_svg: Some(src),
                    float: applied.float.unwrap_or(css::Float::None),
                    clears: applied.clear.unwrap_or(false),
                    position: applied.position.unwrap_or(css::Position::Static),
                    inset: applied_inset(&applied),
                    inset_pct: applied_inset_pct(&applied),
                    width: w,
                    height: h,
                    sprite_crop: None,
                    node: Some(id),
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
        "select" => {
            // A <select> is a dropdown: it shows only its current value, not the
            // whole <option> list. Emit the selected (or first) option's label as
            // an inline run with a disclosure arrow, and skip the option subtree.
            let mut label = String::new();
            let mut first = String::new();
            for &k in &dom.node(id).children {
                if dom.node(k).tag() == Some("option") {
                    let txt = dom.inner_text(k).trim().to_string();
                    if first.is_empty() {
                        first = txt.clone();
                    }
                    if dom.node(k).attr("selected").is_some() {
                        label = txt;
                        break;
                    }
                }
            }
            if label.is_empty() {
                label = first;
            }
            // A styled-select (e.g. Wikipedia's search language picker) hides the
            // native <select> with `color:transparent` and shows its value through
            // a sibling label instead. Unlike the image-replacement trick that
            // `reveal()` surfaces, that text is genuinely redundant here — so when
            // the select's own colour is transparent, emit only the disclosure
            // arrow and let the visible sibling label carry the value.
            let transparent = applied.color.map_or(false, |c| c.3 <= 0.001);
            if transparent {
                push_run(st, "\u{25be}");
            } else if !label.is_empty() {
                push_run(st, &format!("{label} \u{25be}"));
            }
            return;
        }
        "a" => {
            // Inline link: descend so nested formatting (and nested colours) is
            // preserved, tagging every run with this href + the link colour.
            let href = dom.node(id).attr("href").map(|s| s.to_string());
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
            walk_children(dom, id, st);
            st.stack.pop();
            st.cur_color = saved_color;
            st.cur_href = saved_href;
            st.cur_bold = saved_bold;
            st.cur_italic = saved_italic;
            return;
        }
        "button" => {
            // A <button> is a discrete interactive control: reify it as its own box
            // (like a submit `<input>`) so (a) it carries its DOM node — a click routes
            // to *this* element and fires its JS listeners, which folding the label
            // into the surrounding run would lose (the click would hit the parent
            // block instead) — and (b) its background/border render as a button. The
            // label is the flattened text content (nested icon `<i>` sprites are empty).
            flush(st);
            let ty = dom
                .node(id)
                .attr("type")
                .map(|s| s.trim().to_ascii_lowercase())
                .unwrap_or_default();
            if ty == "hidden" {
                return;
            }
            let mut label = dom
                .inner_text(id)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if label.is_empty() {
                label = dom
                    .node(id)
                    .attr("value")
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "Button".to_string());
            }
            // A primary/progressive submit takes its (usually coloured) background and
            // white text; a plain button gets a light chrome with the author text colour.
            let is_submit = ty == "submit";
            let (bg, fg, border_c) = if is_submit {
                (
                    applied.background.unwrap_or((0.2, 0.4, 0.8, 1.0)),
                    applied.color.unwrap_or((1.0, 1.0, 1.0, 1.0)),
                    applied.border_color.unwrap_or((0.15, 0.32, 0.7, 1.0)),
                )
            } else {
                (
                    applied.background.unwrap_or((0.96, 0.96, 0.97, 1.0)),
                    applied.color.unwrap_or(style::LINK),
                    applied.border_color.unwrap_or((0.72, 0.74, 0.78, 1.0)),
                )
            };
            let (mt, mr, mb, ml) = applied.padding_px();
            st.blocks.push(Block::Text {
                text: label,
                size: applied.font_size.unwrap_or(st.style.size),
                color: fg,
                bold: applied.font_weight.unwrap_or(true),
                italic: applied.font_style.unwrap_or(false),
                align: 0.5,
                background: Some(bg),
                margin: applied.margin_px(),
                padding: (mt.max(6.0), mr.max(12.0), mb.max(6.0), ml.max(12.0)),
                width: box_width(&applied, css::Display::InlineBlock),
                height: applied.height.unwrap_or(css::WidthVal::Auto),
                line_height: 0.0,
                border_w: applied.border_width.unwrap_or(1.0),
                border_c,
                spans: Vec::new(),
                clears: false,
                inline_level: true,
                node: Some(id),
            });
            return;
        }
        "input" | "textarea" => {
            // Form field: a real bordered box (text inputs paint white, submit
            // buttons take their background). Without this an `inline-block` control
            // folds into surrounding text and vanishes (e.g. the Wikipedia search
            // field). Its label is the `value`/`placeholder`; an empty text field
            // still gets a line of height so the box shows.
            flush(st);
            let ty = dom
                .node(id)
                .attr("type")
                .map(|s| s.trim().to_ascii_lowercase())
                .unwrap_or_else(|| "text".to_string());
            if ty == "hidden" {
                return;
            }
            let is_button = matches!(ty.as_str(), "submit" | "button" | "reset");
            let mut label = dom
                .node(id)
                .attr("value")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    dom.node(id)
                        .attr("placeholder")
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                })
                .unwrap_or_default();
            if is_button && label.is_empty() {
                label = "Submit".to_string();
            }
            let (bg, fg, border_c) = if is_button {
                (
                    applied.background.unwrap_or((0.2, 0.4, 0.8, 1.0)),
                    applied.color.unwrap_or((1.0, 1.0, 1.0, 1.0)),
                    applied.border_color.unwrap_or((0.15, 0.32, 0.7, 1.0)),
                )
            } else {
                (
                    applied.background.unwrap_or((1.0, 1.0, 1.0, 1.0)),
                    applied.color.unwrap_or((0.2, 0.2, 0.22, 1.0)),
                    applied.border_color.unwrap_or((0.6, 0.62, 0.66, 1.0)),
                )
            };
            let (mt, mr, mb, ml) = applied.padding_px();
            st.blocks.push(Block::Text {
                text: if label.is_empty() {
                    " ".to_string()
                } else {
                    label
                },
                size: applied.font_size.unwrap_or(st.style.size),
                color: fg,
                bold: applied.font_weight.unwrap_or(false),
                italic: applied.font_style.unwrap_or(false),
                align: applied
                    .text_align
                    .map(|a| a.factor())
                    .unwrap_or(if is_button { 0.5 } else { 0.0 }),
                background: Some(bg),
                margin: applied.margin_px(),
                padding: (mt.max(6.0), mr.max(10.0), mb.max(6.0), ml.max(10.0)),
                width: box_width(&applied, display),
                height: applied.height.unwrap_or(css::WidthVal::Auto),
                line_height: 0.0,
                border_w: applied.border_width.unwrap_or(1.0),
                border_c,
                spans: Vec::new(),
                clears: false,
                inline_level: true,
                node: Some(id),
            });
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
        collect_table_rows(dom, id, st, &mut rows);
        // A table draws grid lines / cell fills only when it asks for a border —
        // via CSS `border` or the legacy `border` attribute. Borderless tables are
        // pure layout scaffolding (google's search form, many legacy pages), which
        // a browser renders invisibly; drawing our default grey grid there boxes up
        // the whole page. Match that: styled only when bordered.
        let bordered = applied.border_width.map(|w| w > 0.0).unwrap_or(false)
            || dom
                .node(id)
                .attr("border")
                .map(|b| {
                    let b = b.trim();
                    !b.is_empty() && b != "0"
                })
                .unwrap_or(false);
        let grid = table::Grid { rows, bordered };
        if grid.rows.iter().any(|r| !r.cells.is_empty()) {
            st.blocks.push(table::build(grid));
        }
        return;
    }

    if display == css::Display::Grid {
        // Grid container: collect its children (like the flex path) and hand them
        // to the grid builder, which flows them row-major into the columns from
        // `grid-template-columns`. Without an explicit template we can't know the
        // column count, so fall through to normal block flow (single column).
        if let Some(weights) = applied.grid_columns.clone() {
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
            walk_children(dom, id, st);
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
                st.blocks.push(crate::grid::build(
                    children,
                    &weights,
                    applied.gap.unwrap_or(0.0),
                    applied.margin_px(),
                    applied.padding_px(),
                    applied.background,
                    applied.float.unwrap_or(css::Float::None),
                    applied.clear.unwrap_or(false),
                    applied.position.unwrap_or(css::Position::Static),
                    applied_inset(&applied),
                    applied_inset_pct(&applied),
                    Some(id),
                ));
            }
            return;
        }
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
        walk_children(dom, id, st);
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
                inset_pct: applied_inset_pct(&applied),
                width: applied.width.unwrap_or(css::WidthVal::Auto),
                height: applied.height.unwrap_or(css::WidthVal::Auto),
                node: Some(id),
                center: wants_center(&applied),
                max_height: applied.max_height,
                inline_level: false,
            });
        }
        return;
    }

    // Positioned block (non-flex, non-static): a discrete box that the engine
    // shifts (`relative`) or pulls out of flow and anchors (`absolute`). Made a
    // container so an `absolute` descendant resolves against it. `absolute`/`fixed`
    // take precedence over float (CSS ignores float on out-of-flow boxes), but a
    // *relatively*-positioned float still floats (relative only offsets it after
    // layout) — so route a `position:relative` + `float` box to the float branch
    // below (e.g. the footer's `.other-project{float:left;position:relative}`
    // cards, which use `relative` only as a positioning context, not an offset).
    let position = applied.position.unwrap_or(css::Position::Static);
    let float_side = applied.float.unwrap_or(css::Float::None);
    let positioned_here = match position {
        css::Position::Absolute => true,
        css::Position::Relative => float_side == css::Float::None,
        css::Position::Static => false,
    };
    if positioned_here {
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
        walk_children(dom, id, st);
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
                inset_pct: applied_inset_pct(&applied),
                width: box_width(&applied, display),
                height: applied.height.unwrap_or(css::WidthVal::Auto),
                node: Some(id),
                center: wants_center(&applied),
                max_height: applied.max_height,
                // An `inline-block` that is also positioned (e.g. the portal's
                // `.search-input`: `display:inline-block;position:relative`) is
                // still an inline-level box — mark it so it groups into a row with
                // its inline siblings (the search field beside the Search button).
                inline_level: display == css::Display::InlineBlock,
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
        walk_children(dom, id, st);
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
                inset_pct: (None, None, None, None),
                width: applied.width.unwrap_or(css::WidthVal::Auto),
                height: applied.height.unwrap_or(css::WidthVal::Auto),
                node: Some(id),
                center: false,
                max_height: applied.max_height,
                inline_level: false,
            });
        }
        return;
    }

    if display == css::Display::Inline {
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
        walk_children(dom, id, st);
        st.stack.pop();
        st.cur_color = saved_color;
        st.cur_bold = saved_bold;
        st.cur_italic = saved_italic;
        return;
    }

    if display == css::Display::InlineBlock {
        // An `inline-block` with a definite size (an explicit width, or a
        // max-width) is a structural inline-level box — the footer's
        // `.other-project-icon{width:50px}` / `.other-project-text{max-width:65%}`
        // cells, the language picker, and so on. Reify it as its own box (tagged
        // `inline_level`) so its size takes effect and [`group_inline_blocks`]
        // can lay it beside its inline-level siblings. A width-less inline-block
        // is just an inline run (an inline-styled span/label), so fall through to
        // the inline fold below.
        let structural = matches!(
            applied.width,
            Some(css::WidthVal::Px(_)) | Some(css::WidthVal::Pct(_))
        ) || applied.max_width.is_some();
        if structural {
            flush(st);
            let saved_style = st.style;
            let saved_align = st.cur_align;
            let saved_color = st.cur_color;
            let saved_bold = st.cur_bold;
            let saved_italic = st.cur_italic;
            let saved_href = st.cur_href.clone();
            let saved_line_height = st.cur_line_height;
            let saved_block_owner = st.block_owner;
            st.block_owner = Some(id);
            if let Some(c) = applied.color {
                st.style.color = c;
                st.cur_color = c;
            }
            if let Some(s) = applied.font_size {
                st.style.size = s;
            }
            st.cur_line_height = applied.line_height.or(st.cur_line_height);
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
            walk_children(dom, id, st);
            st.stack.pop();
            flush(st);
            let children = std::mem::replace(&mut st.blocks, saved_blocks);

            st.style = saved_style;
            st.cur_align = saved_align;
            st.cur_color = saved_color;
            st.cur_bold = saved_bold;
            st.cur_italic = saved_italic;
            st.cur_href = saved_href;
            st.cur_line_height = saved_line_height;
            st.block_owner = saved_block_owner;

            if !children.is_empty() {
                st.blocks.push(Block::Container {
                    direction: css::FlexDirection::Column,
                    justify: css::Justify::Start,
                    align: css::Align::Start,
                    gap: 0.0,
                    margin: applied.margin_px(),
                    padding: applied.padding_px(),
                    background: applied.background,
                    children,
                    float: css::Float::None,
                    clears: applied.clear.unwrap_or(false),
                    position: css::Position::Static,
                    inset: (None, None, None, None),
                    inset_pct: (None, None, None, None),
                    width: applied.width.unwrap_or(css::WidthVal::Auto),
                    height: applied.height.unwrap_or(css::WidthVal::Auto),
                    node: Some(id),
                    center: false,
                    max_height: applied.max_height,
                    inline_level: true,
                });
            }
            return;
        }

        // Width-less inline-block: fold into the current run like a plain inline.
        let saved_color = st.cur_color;
        let saved_bold = st.cur_bold;
        let saved_italic = st.cur_italic;
        if let Some(c) = applied.color {
            st.cur_color = c;
        }
        if let Some(w) = applied.font_weight {
            st.cur_bold = w;
        }
        if let Some(i) = applied.font_style {
            st.cur_italic = i;
        }
        st.stack.push(ctx);
        walk_children(dom, id, st);
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
        let saved_block_owner = st.block_owner;
        st.block_owner = Some(id);

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
        st.cur_line_height = applied.line_height.or(st.cur_line_height);
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
        walk_children(dom, id, st);
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
        st.block_owner = saved_block_owner;
        return;
    }

    // Sized / centered block box: a plain block-level div with a definite CSS
    // width, or the `margin:0 auto` centering idiom. Unlike the generic flow
    // boundary below (which drops the box model), this reifies a container so the
    // width, centering, padding and background take effect — e.g. the Wikipedia
    // `.search-container{width:540px;margin:.4rem auto;text-align:center}`, which
    // otherwise spreads full-width. Lists keep the generic path so their `<li>`
    // marker context is preserved.
    if !matches!(name, "ul" | "ol")
        && (wants_center(&applied)
            || matches!(
                applied.width,
                Some(css::WidthVal::Px(_)) | Some(css::WidthVal::Pct(_))
            ))
    {
        flush(st);
        let saved_style = st.style;
        let saved_align = st.cur_align;
        let saved_color = st.cur_color;
        let saved_bold = st.cur_bold;
        let saved_italic = st.cur_italic;
        let saved_href = st.cur_href.clone();
        let saved_line_height = st.cur_line_height;
        let saved_block_owner = st.block_owner;
        st.block_owner = Some(id);
        if let Some(c) = applied.color {
            st.style.color = c;
            st.cur_color = c;
        }
        if let Some(s) = applied.font_size {
            st.style.size = s;
        }
        st.cur_line_height = applied.line_height.or(st.cur_line_height);
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
        walk_children(dom, id, st);
        st.stack.pop();
        flush(st);
        let children = std::mem::replace(&mut st.blocks, saved_blocks);

        st.style = saved_style;
        st.cur_align = saved_align;
        st.cur_color = saved_color;
        st.cur_bold = saved_bold;
        st.cur_italic = saved_italic;
        st.cur_href = saved_href;
        st.cur_line_height = saved_line_height;
        st.block_owner = saved_block_owner;

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
                position: css::Position::Static,
                inset: (None, None, None, None),
                inset_pct: (None, None, None, None),
                width: applied.width.unwrap_or(css::WidthVal::Auto),
                height: applied.height.unwrap_or(css::WidthVal::Auto),
                node: Some(id),
                center: wants_center(&applied),
                max_height: applied.max_height,
                inline_level: false,
            });
        }
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
    let saved_block_owner = st.block_owner;
    st.block_owner = Some(id);
    if let Some(c) = applied.color {
        st.style.color = c;
        st.cur_color = c;
    }
    if let Some(s) = applied.font_size {
        st.style.size = s;
    }
    // `line-height` inherits down to descendant text blocks.
    st.cur_line_height = applied.line_height.or(st.cur_line_height);
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
    // Legacy `<center>` centres *block* descendants too (it maps to
    // `text-align:-moz-center`), not just inline content: the logo `<div>` and the
    // search `<form>` shrink to their content and sit centred. We reify it as a
    // full-width Column whose cross-axis alignment is Center, so auto-width block
    // children shrink-to-fit and centre; `cur_align` also centres any loose text.
    let is_center = name == "center";
    if is_center {
        st.cur_align = 0.5;
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
    // A `text-align:center`/`right` block centres (or right-aligns) its inline
    // content — including a reified inline box like the portal's centred
    // `.lang-list-button-wrapper > button` ("Read Wikipedia in your language").
    // Full-width text blocks carry their own `align` factor, so they don't need
    // this; but a shrink-to-fit inline box does, so when such a box is present we
    // divert the children and wrap them in a full-width row that justifies them to
    // the requested edge. Plain (all-block) content keeps the flat flow.
    let align_edge = match applied.text_align {
        Some(css::TextAlign::Center) => Some(css::Justify::Center),
        Some(css::TextAlign::Right) => Some(css::Justify::End),
        _ => None,
    };
    let divert = is_center || align_edge.is_some();
    let saved_blocks = divert.then(|| std::mem::take(&mut st.blocks));
    st.stack.push(ctx);
    walk_children(dom, id, st);
    st.stack.pop();
    if is_list {
        st.lists.pop();
    }
    flush(st);
    if is_center {
        if let Some(saved) = saved_blocks {
            let children = std::mem::replace(&mut st.blocks, saved);
            if !children.is_empty() {
                st.blocks.push(Block::Container {
                    direction: css::FlexDirection::Column,
                    justify: css::Justify::Start,
                    align: css::Align::Center,
                    gap: 0.0,
                    margin: applied.margin_px(),
                    padding: applied.padding_px(),
                    background: applied.background,
                    children,
                    float: css::Float::None,
                    clears: applied.clear.unwrap_or(false),
                    position: css::Position::Static,
                    inset: (None, None, None, None),
                    inset_pct: (None, None, None, None),
                    width: css::WidthVal::Pct(1.0),
                    height: css::WidthVal::Auto,
                    node: Some(id),
                    center: false,
                    max_height: None,
                    inline_level: false,
                });
            }
        }
    } else if let (Some(justify), Some(saved)) = (align_edge, saved_blocks) {
        let mut children = std::mem::replace(&mut st.blocks, saved);
        if children.iter().any(is_inline_level) {
            st.blocks.push(Block::Container {
                direction: css::FlexDirection::Row,
                justify,
                align: css::Align::Center,
                gap: 6.0,
                margin: (0.0, 0.0, 0.0, 0.0),
                padding: (0.0, 0.0, 0.0, 0.0),
                background: None,
                children,
                float: css::Float::None,
                clears: false,
                position: css::Position::Static,
                inset: (None, None, None, None),
                inset_pct: (None, None, None, None),
                width: css::WidthVal::Pct(1.0),
                height: css::WidthVal::Auto,
                node: Some(id),
                center: false,
                max_height: None,
                inline_level: false,
            });
        } else {
            st.blocks.append(&mut children);
        }
    }
    st.style = saved_style;
    st.cur_align = saved_align;
    st.cur_color = saved_color;
    st.cur_bold = saved_bold;
    st.cur_italic = saved_italic;
    st.cur_clears = saved_clears;
    st.cur_line_height = saved_line_height;
    st.block_owner = saved_block_owner;
}

/// Parse a `colspan`/`rowspan`-style positive integer attribute.
fn attr_usize(node: &crate::dom::Node, key: &str) -> Option<usize> {
    node.attr(key)
        .and_then(|s| s.trim().parse::<usize>().ok())
        .filter(|&n| n >= 1)
}

/// Collect a table's rows, descending through `<thead>`/`<tbody>`/`<tfoot>`
/// grouping wrappers so their `<tr>`s land in document order.
fn collect_table_rows(
    dom: &crate::dom::Dom,
    id: crate::dom::NodeId,
    st: &mut State,
    rows: &mut Vec<table::GridRow>,
) {
    let kids = dom.node(id).children.clone();
    for h in kids {
        let Some(name) = dom.node(h).tag() else {
            continue;
        };
        match name {
            "tr" => rows.push(parse_table_row(dom, h, st)),
            "thead" | "tbody" | "tfoot" => collect_table_rows(dom, h, st, rows),
            _ => {}
        }
    }
}

/// Parse one `<tr>` into a row of cells (`<td>`/`<th>`).
fn parse_table_row(dom: &crate::dom::Dom, id: crate::dom::NodeId, st: &mut State) -> table::GridRow {
    let mut cells = Vec::new();
    let kids = dom.node(id).children.clone();
    for h in kids {
        let Some(name) = dom.node(h).tag() else {
            continue;
        };
        match name {
            "td" => cells.push(parse_table_cell(dom, h, st, false)),
            "th" => cells.push(parse_table_cell(dom, h, st, true)),
            _ => {}
        }
    }
    table::GridRow { cells }
}

/// Parse a single cell: walk its children into a fresh block list under a fresh
/// inline style (headers are bold + centred), then measure its min/max content
/// width for the column algorithm.
fn parse_table_cell(
    dom: &crate::dom::Dom,
    id: crate::dom::NodeId,
    st: &mut State,
    header: bool,
) -> table::Cell {
    let colspan = attr_usize(dom.node(id), "colspan").unwrap_or(1);
    let rowspan = attr_usize(dom.node(id), "rowspan").unwrap_or(1);

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

    let ctx = elem_ctx(dom, id, if header { "th" } else { "td" });
    st.stack.push(ctx);
    walk_children(dom, id, st);
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

fn walk_children(dom: &crate::dom::Dom, id: crate::dom::NodeId, st: &mut State) {
    // Copy child ids out first (NodeId is Copy) so the immutable borrow of the
    // parent node doesn't overlap the recursive mutation of `st`.
    let kids = dom.node(id).children.clone();
    for cid in kids {
        walk(dom, cid, st);
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

/// A colour is "revealed" if it's fully transparent: such text is invisible in a
/// normal render, but on the web `color:transparent` is the classic
/// image-replacement trick (e.g. the Wikipedia wordmark, a `<span>` reading
/// "Wikipedia" hidden behind an SVG sprite). We can't draw the sprite, so surface
/// the accessible text in the default body colour instead of dropping it.
fn reveal(c: crate::style::Rgba) -> crate::style::Rgba {
    if c.3 <= 0.001 {
        crate::style::BODY_TEXT
    } else {
        c
    }
}

/// Emit the pending block as a Text block. `buf` is already collapsed/trimmed.
fn flush(st: &mut State) {
    let text = std::mem::take(&mut st.buf);
    let mut spans = std::mem::take(&mut st.spans);
    for s in &mut spans {
        s.color = reveal(s.color);
    }
    st.pending_space = false;
    if !text.is_empty() {
        st.blocks.push(Block::Text {
            text,
            size: st.style.size,
            color: reveal(st.style.color),
            bold: st.style.bold,
            italic: st.style.italic,
            align: st.cur_align,
            background: st.cur_bg,
            margin: st.cur_margin,
            padding: st.cur_padding,
            width: st.cur_width,
            height: st.cur_height,
            // Resolve the inherited spec against this block's own font-size: a
            // unitless multiplier scales per element (so a 30px wordmark gets a
            // 45px line box, not the body's), a px length passes through.
            line_height: match st.cur_line_height {
                Some(css::LineHeight::Scale(m)) => m * st.style.size,
                Some(css::LineHeight::Px(px)) => px,
                None => 0.0,
            },
            border_w: st.cur_border_w,
            border_c: st.cur_border_c,
            spans,
            clears: st.cur_clears,
            inline_level: false,
            node: st.block_owner,
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
