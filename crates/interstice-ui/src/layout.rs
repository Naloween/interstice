use crate::text::*;
use crate::types::*;
use std::collections::BTreeMap;

type ClipRect = (f32, f32, f32, f32);

/// Per-pass memo of intrinsic sizes. Flow layout measures each element's fit
/// width and content height several times per parent — once for the parent's own
/// size, again in the fixed/grow distribution fold, and again in the recursive
/// child layout — and each of those re-descends the whole subtree. Un-memoised
/// that is roughly O(n·depth) text-shaping calls *per frame*, which is what makes
/// a deep page (e.g. a long Wikipedia article) crawl. Both `fit_width` and
/// `fit_height` are pure functions of `(element, width)` over the immutable `all`
/// slice within a single layout pass, so caching them by element identity (the
/// element's address, stable for the pass) plus the width they were measured
/// against (results legitimately differ per width — percentage descendants) is
/// exact, collapsing the redundant re-measurement to O(n).
#[derive(Default)]
struct Measure {
    width: BTreeMap<(usize, u32), f32>,
    height: BTreeMap<(usize, u32), f32>,
}

#[inline]
fn cache_key(el: &UiElement, w: f32) -> (usize, u32) {
    (el as *const UiElement as usize, w.to_bits())
}

/// State threaded through a single layout pass: the intrinsic-size memo plus a
/// parent→children adjacency index.
///
/// Without the index every `fit_width`/`fit_height`/`layout_node` call rebuilt
/// its child list by scanning the *entire* element slice (`all.iter().filter(
/// parent == id)`) — an O(n) sweep performed once per element, i.e. O(n²) per
/// frame. On a long page (thousands of nodes) that quadratic dominates and the
/// frame rate collapses even though nothing about the page changed. Building the
/// adjacency once up front (O(n), children pre-filtered to `visible` and
/// pre-sorted by `order`, exactly as the old inline scans did) turns every child
/// lookup into an O(1) map hit, making a full pass linear in the node count.
struct LayoutPass<'a> {
    /// Visible children of each element id, pre-sorted by `order`.
    kids: BTreeMap<&'a str, Vec<&'a UiElement>>,
    measure: Measure,
}

impl<'a> LayoutPass<'a> {
    fn new(all: &'a [UiElement]) -> Self {
        let mut kids: BTreeMap<&'a str, Vec<&'a UiElement>> = BTreeMap::new();
        for el in all {
            if !el.visible {
                continue;
            }
            if let Some(p) = el.parent.as_deref() {
                kids.entry(p).or_default().push(el);
            }
        }
        for v in kids.values_mut() {
            v.sort_by_key(|c| c.order);
        }
        LayoutPass {
            kids,
            measure: Measure::default(),
        }
    }

    /// Visible children of `id`, sorted by `order`. Returns an owned `Vec` of
    /// references (cheap pointer copies into the immutable `all` slice) so the
    /// borrow of `self` ends immediately and the caller can keep recursing with
    /// `&mut self`.
    fn children(&self, id: &str) -> Vec<&'a UiElement> {
        self.kids.get(id).cloned().unwrap_or_default()
    }
}

/// The containing block for positioned (`Absolute`) descendants: `rect` is the
/// content box `(x, y, w, h)` they anchor against; `clip` is the clip region
/// they're bounded by. Threaded down through layout so a positioned element
/// overrides it for its subtree while `Static` elements pass it through (so an
/// absolute box resolves against its nearest positioned ancestor).
#[derive(Clone, Copy)]
pub struct ContainingBlock {
    pub rect: ClipRect,
    pub clip: ClipRect,
}

/// The whole-surface containing block used for root elements.
pub fn surface_cb(sw: f32, sh: f32) -> ContainingBlock {
    let s = (0.0, 0.0, sw, sh);
    ContainingBlock { rect: s, clip: s }
}

/// In-flow shift for a `Relative` element from its `pos_*` offsets: prefer
/// left/top, fall back to the negated right/bottom, else 0. Percentage offsets
/// resolve against the containing block's width (`cbw`) / height (`cbh`).
fn relative_shift(el: &UiElement, cbw: f32, cbh: f32) -> (f32, f32) {
    let dx = el
        .eff_left(cbw)
        .or_else(|| el.eff_right(cbw).map(|r| -r))
        .unwrap_or(0.0);
    let dy = el
        .eff_top(cbh)
        .or_else(|| el.eff_bottom(cbh).map(|b| -b))
        .unwrap_or(0.0);
    (dx, dy)
}

pub struct ComputedElement<'a> {
    pub schema: &'a UiElement,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub clip: ClipRect,
}

/// Clamp a computed used height to the element's CSS `max-height`, if any. In-flow
/// children are already clipped to the parent's content box, so capping the box
/// height here is all that `max-height` (with the usual `overflow:hidden`) needs:
/// `max-height:0` collapses the box to nothing, a larger cap expands it.
fn cap_height(el: &UiElement, h: f32) -> f32 {
    match el.max_height {
        Some(m) => h.min(m.max(0.0)),
        None => h,
    }
}

fn intersect_clip(a: ClipRect, b: ClipRect) -> ClipRect {
    let x1 = a.0.max(b.0);
    let y1 = a.1.max(b.1);
    let x2 = (a.0 + a.2).min(b.0 + b.2);
    let y2 = (a.1 + a.3).min(b.1 + b.3);
    (x1, y1, (x2 - x1).max(0.0), (y2 - y1).max(0.0))
}

fn fit_width<'a>(el: &'a UiElement, avail_w: f32, p: &mut LayoutPass<'a>) -> f32 {
    let k = cache_key(el, avail_w);
    if let Some(&v) = p.measure.width.get(&k) {
        return v;
    }

    let text_min_w = el
        .text
        .as_deref()
        .map(|t| min_text_width(t, el.text_size, &el.text_wrap, el.base_font_style()))
        .unwrap_or(0.0);

    // Absolute children are out of flow: they don't contribute to the parent's
    // intrinsic size (CSS). Measuring them here would inflate the box — e.g. the
    // portal globe (position:absolute) would add its height to `.central-textlogo`.
    let children: Vec<&UiElement> = p
        .children(&el.id)
        .into_iter()
        .filter(|c| c.position != Position::Absolute)
        .collect();

    let inner_avail = (avail_w - el.pad_x()).max(0.0);
    let visible_n = children.len();
    let gap_total = if visible_n > 1 {
        el.gap * (visible_n - 1) as f32
    } else {
        0.0
    };

    let children_w = match el.layout_direction {
        LayoutDirection::Row => {
            let sum: f32 = children
                .iter()
                .map(|c| child_min_w(c, inner_avail, p) + c.mrg_x())
                .sum();
            sum + gap_total
        }
        LayoutDirection::Column => children
            .iter()
            .map(|c| child_min_w(c, inner_avail, p) + c.mrg_x())
            .fold(0.0f32, f32::max),
    };

    let result = (text_min_w.max(children_w) + el.pad_x()).max(0.0);
    p.measure.width.insert(k, result);
    result
}

fn child_min_w<'a>(child: &'a UiElement, parent_inner_avail: f32, p: &mut LayoutPass<'a>) -> f32 {
    match child.width {
        Size::Fixed(px) => px.max(0.0),
        Size::Percent(f) => (parent_inner_avail * f).max(0.0),
        Size::Grow | Size::Fit => fit_width(child, parent_inner_avail, p),
    }
}

fn fit_height<'a>(el: &'a UiElement, inner_w: f32, p: &mut LayoutPass<'a>) -> f32 {
    let k = cache_key(el, inner_w);
    if let Some(&v) = p.measure.height.get(&k) {
        return v;
    }

    let text_h = el
        .text
        .as_deref()
        .map(|t| {
            let base = el.base_font_style();
            let lines = compute_lines(t, el.text_size, inner_w, &el.text_wrap, base);
            lines.len() as f32 * el.line_height_px()
        })
        .unwrap_or(0.0);

    // Absolute children are out of flow and don't contribute to the parent's
    // intrinsic height (see `fit_width`).
    let children: Vec<&UiElement> = p
        .children(&el.id)
        .into_iter()
        .filter(|c| c.position != Position::Absolute)
        .collect();

    let visible_n = children.len();
    let gap_total = if visible_n > 1 {
        el.gap * (visible_n - 1) as f32
    } else {
        0.0
    };

    let children_h = match el.layout_direction {
        LayoutDirection::Column => {
            let sum: f32 = children
                .iter()
                .map(|c| child_resolved_h(c, inner_w, p) + c.mrg_y())
                .sum();
            sum + gap_total
        }
        LayoutDirection::Row => children
            .iter()
            .map(|c| child_resolved_h(c, inner_w, p) + c.mrg_y())
            .fold(0.0f32, f32::max),
    };

    let result = (text_h.max(children_h) + el.pad_y()).max(0.0);
    p.measure.height.insert(k, result);
    result
}

fn child_resolved_h<'a>(child: &'a UiElement, parent_inner_w: f32, p: &mut LayoutPass<'a>) -> f32 {
    let child_outer_w = match child.width {
        Size::Fixed(px) => px.max(0.0),
        Size::Percent(f) => (parent_inner_w * f).max(0.0),
        Size::Grow | Size::Fit => {
            let min_w = fit_width(child, parent_inner_w, p);
            (parent_inner_w - child.mrg_x()).max(0.0).max(min_w)
        }
    };
    let child_inner_w = (child_outer_w - child.pad_x()).max(0.0);
    let h = match child.height {
        Size::Fixed(px) => px.max(0.0),
        // A percent height against an indefinite (content-sized) parent resolves
        // to auto in CSS — treat it like Fit/Grow here.
        Size::Percent(_) | Size::Grow | Size::Fit => fit_height(child, child_inner_w, p),
    };
    // Cap so the parent reserves only the (possibly collapsed) used height — keeps
    // flow measurement consistent with the capped box laid out in `layout_node`.
    cap_height(child, h)
}

/// Lay out `el` and its subtree. Public entry point: allocates a fresh per-pass
/// [`LayoutPass`] (adjacency index + intrinsic-size memo; each root is
/// independent) and delegates to [`layout_node`], which threads it through the
/// recursion.
#[allow(clippy::too_many_arguments)]
pub fn layout_element<'a>(
    all: &'a [UiElement],
    el: &'a UiElement,
    origin_x: f32,
    origin_y: f32,
    avail_w: f32,
    avail_h: f32,
    clip: ClipRect,
    cb: ContainingBlock,
) -> Vec<ComputedElement<'a>> {
    let mut p = LayoutPass::new(all);
    layout_node(el, origin_x, origin_y, avail_w, avail_h, clip, cb, &mut p)
}

#[allow(clippy::too_many_arguments)]
/// Shift a laid-out subtree by `(dx, dy)`. Used to move a child to its final
/// cross-axis position (align/centre) without re-running layout: a translation
/// moves every box and its clip rect uniformly, which is exactly what a second
/// layout at the offset origin would have produced.
fn translate_nodes(nodes: &mut [ComputedElement], dx: f32, dy: f32) {
    for n in nodes {
        n.x += dx;
        n.y += dy;
        n.clip.0 += dx;
        n.clip.1 += dy;
    }
}

fn layout_node<'a>(
    el: &'a UiElement,
    origin_x: f32,
    origin_y: f32,
    avail_w: f32,
    avail_h: f32,
    clip: ClipRect,
    cb: ContainingBlock,
    p: &mut LayoutPass<'a>,
) -> Vec<ComputedElement<'a>> {
    if !el.visible {
        return Vec::new();
    }

    // `Relative` shifts this box (and its subtree) without affecting siblings:
    // the parent still advances by the element's flow size, computed from width/
    // height, not position.
    let (rel_dx, rel_dy) = if el.position == Position::Relative {
        let (_, _, cbw, cbh) = cb.rect;
        relative_shift(el, cbw, cbh)
    } else {
        (0.0, 0.0)
    };
    let x = origin_x + el.mrg_l() + rel_dx;
    let y = origin_y + el.mrg_t() + rel_dy;

    let own_w = match el.width {
        Size::Fixed(px) => px.max(0.0),
        Size::Percent(f) => (avail_w * f).max(0.0),
        Size::Grow => (avail_w - el.mrg_x())
            .max(0.0)
            .max(fit_width(el, avail_w, p)),
        Size::Fit => fit_width(el, avail_w, p),
    };
    let inner_w = (own_w - el.pad_x()).max(0.0);

    let own_h = match el.height {
        Size::Fixed(px) => px.max(0.0),
        Size::Percent(f) => (avail_h * f).max(0.0),
        Size::Grow => (avail_h - el.mrg_y())
            .max(0.0)
            .max(fit_height(el, inner_w, p)),
        Size::Fit => fit_height(el, inner_w, p),
    };
    let own_h = cap_height(el, own_h);
    let inner_h = (own_h - el.pad_y()).max(0.0);

    let self_clip = intersect_clip(clip, (x, y, own_w, own_h));
    // Content clip also accounts for scroll offset (children are shifted up/left by scroll).
    let content_clip = intersect_clip(
        self_clip,
        (x + el.pad_l(), y + el.pad_t(), inner_w, inner_h),
    );

    let all_children = p.children(&el.id);
    // Absolute children are out of flow: they don't drive the cursor and are
    // positioned separately against the containing block.
    let (abs_children, children): (Vec<&UiElement>, Vec<&UiElement>) = all_children
        .into_iter()
        .partition(|c| c.position == Position::Absolute);

    let (fixed_main, grow_count) =
        children.iter().fold((0.0f32, 0u32), |(acc, gc), child| {
            match el.layout_direction {
                LayoutDirection::Row => match child.width {
                    Size::Grow => (acc, gc + 1),
                    Size::Fixed(px) => (acc + px.max(0.0) + child.mrg_x(), gc),
                    Size::Percent(f) => (acc + (inner_w * f).max(0.0) + child.mrg_x(), gc),
                    Size::Fit => {
                        let w = fit_width(child, inner_w, p);
                        (acc + w + child.mrg_x(), gc)
                    }
                },
                LayoutDirection::Column => match child.height {
                    Size::Grow => (acc, gc + 1),
                    Size::Fixed(px) => (acc + px.max(0.0) + child.mrg_y(), gc),
                    Size::Percent(f) => (acc + (inner_h * f).max(0.0) + child.mrg_y(), gc),
                    Size::Fit => {
                        let h = child_resolved_h(child, inner_w, p);
                        (acc + h + child.mrg_y(), gc)
                    }
                },
            }
        });

    let visible_n = children.len() as f32;
    let total_gap = if visible_n > 1.0 {
        el.gap * (visible_n - 1.0)
    } else {
        0.0
    };
    let remaining = match el.layout_direction {
        LayoutDirection::Row => (inner_w - fixed_main - total_gap).max(0.0),
        LayoutDirection::Column => (inner_h - fixed_main - total_gap).max(0.0),
    };
    let grow_size = if grow_count > 0 {
        remaining / grow_count as f32
    } else {
        0.0
    };

    // Apply scroll offset to child origin.
    let scroll_ox = if el.scrollable_x { el.scroll_x } else { 0.0 };
    let scroll_oy = if el.scrollable_y { el.scroll_y } else { 0.0 };

    // Main-axis distribution (CSS `justify-content`). Only meaningful when a
    // grow child hasn't already consumed all the slack.
    let (lead, extra_gap) = if grow_count == 0 && remaining > 0.0 {
        justify_offsets(&el.justify_content, remaining, children.len())
    } else {
        (0.0, 0.0)
    };
    let inner_cross = match el.layout_direction {
        LayoutDirection::Row => inner_h,
        LayoutDirection::Column => inner_w,
    };
    // Cross extent used to centre a `margin:0 auto` child. A `Grow` box takes the
    // greater of its available width and its content's — so an oversized child
    // (a wide float grid, a nowrap band) inflates `inner_cross` past the viewport.
    // CSS never lets overflow widen a block's content box, so centre within the
    // *available* extent, not the inflated one; otherwise every centred sibling
    // (e.g. the portal's `.central-featured`) drifts right by half the overflow.
    // Fixed/Percent boxes are already un-inflated, so keep their inner extent.
    let center_extent = match el.layout_direction {
        LayoutDirection::Column => match el.width {
            Size::Grow => ((avail_w - el.mrg_x()).max(0.0) - el.pad_x()).max(0.0),
            _ => inner_cross,
        },
        LayoutDirection::Row => match el.height {
            Size::Grow => ((avail_h - el.mrg_y()).max(0.0) - el.pad_y()).max(0.0),
            _ => inner_cross,
        },
    };

    let mut cursor = lead;
    let content_x = x + el.pad_l() - scroll_ox;
    let content_y = y + el.pad_t() - scroll_oy;

    // Containing block handed to descendants: a positioned element (relative or
    // absolute) becomes the anchor for its absolute descendants; otherwise the
    // inherited containing block passes straight through.
    //
    // `clip` here bounds those absolute descendants (it is consumed only by
    // `layout_absolute`). CSS `overflow` defaults to `visible`, so a positioned
    // box must NOT clip an overflowing absolute descendant to its own — possibly
    // short — content box (e.g. an `absolute` logo taller than its `relative`
    // wrapper). Only a scroll container clips; a non-scroll box hands down the
    // clip it is itself bounded by, so the descendant stays visible.
    let child_cb = if el.position == Position::Static {
        cb
    } else {
        let abs_clip = if el.scrollable_x || el.scrollable_y {
            content_clip
        } else {
            clip
        };
        ContainingBlock {
            rect: (content_x, content_y, inner_w, inner_h),
            clip: abs_clip,
        }
    };

    let mut result = Vec::new();
    result.push(ComputedElement {
        schema: el,
        x,
        y,
        width: own_w,
        height: own_h,
        clip: self_clip,
    });

    for child in &children {
        // Main-axis available size handed to the child: a `Grow` child gets its
        // distributed share (`grow_size`), but a `Percent` child must resolve its
        // percentage against the parent's full inner extent — its containing block
        // — not the grow slack (which is 0 when no sibling grows, collapsing e.g.
        // the footer's `width:33%` cards to nothing). Fixed/Fit are unaffected by
        // the value, so keep them on `grow_size` as before.
        let (child_avail_w, child_avail_h) = match el.layout_direction {
            LayoutDirection::Row => {
                let main = match child.width {
                    Size::Percent(_) => inner_w,
                    _ => grow_size,
                };
                (main, inner_h)
            }
            LayoutDirection::Column => {
                let main = match child.height {
                    Size::Percent(_) => inner_h,
                    _ => grow_size,
                };
                (inner_w, main)
            }
        };
        let (child_origin_x, child_origin_y) = match el.layout_direction {
            LayoutDirection::Row => (content_x + cursor, content_y),
            LayoutDirection::Column => (content_x, content_y + cursor),
        };

        // Lay out once at the cross-start. For `Center`/`End` we then need the
        // child's resolved cross size to compute its offset and re-lay it out at
        // the final origin so its clip is computed correctly (Start/Stretch keep
        // the single-pass path).
        let mut child_nodes = layout_node(
            child,
            child_origin_x,
            child_origin_y,
            child_avail_w,
            child_avail_h,
            content_clip,
            child_cb,
            p,
        );

        let child_cross = match el.layout_direction {
            LayoutDirection::Row => child_nodes
                .first()
                .map(|c| c.height + child.mrg_y())
                .unwrap_or(0.0),
            LayoutDirection::Column => child_nodes
                .first()
                .map(|c| c.width + child.mrg_x())
                .unwrap_or(0.0),
        };
        // `center_cross` (CSS `margin: 0 auto` on a definite-width block) centres
        // this child alone, overriding the parent's `align_items`.
        let cross_off = if child.center_cross {
            (center_extent - child_cross).max(0.0) / 2.0
        } else {
            cross_offset(&el.align_items, inner_cross, child_cross)
        };
        if cross_off > 0.0 {
            // The final origin differs from the first pass by a pure cross-axis
            // translation, so shift the already-computed subtree rather than
            // re-laying it out. Re-invoking `layout_node` here would recompute the
            // child's entire subtree a second time, and since each of *its*
            // children could do the same, layout would cost O(2^depth) — a deeply
            // nested page (e.g. a Wikipedia article's stacked tables) would explode
            // into billions of boxes and exhaust memory. Translation is O(subtree).
            let (dx, dy) = match el.layout_direction {
                LayoutDirection::Row => (0.0, cross_off),
                LayoutDirection::Column => (cross_off, 0.0),
            };
            translate_nodes(&mut child_nodes, dx, dy);
        }

        let child_main = match el.layout_direction {
            LayoutDirection::Row => child_nodes
                .first()
                .map(|c| c.width + child.mrg_x())
                .unwrap_or(0.0),
            LayoutDirection::Column => child_nodes
                .first()
                .map(|c| c.height + child.mrg_y())
                .unwrap_or(0.0),
        };

        cursor += child_main + el.gap + extra_gap;
        result.extend(child_nodes);
    }

    // Out-of-flow absolute children, positioned against `child_cb` and painted
    // after the in-flow content (so they sit on top, matching CSS paint order).
    for child in &abs_children {
        let nodes = layout_absolute(child, child_cb, p);
        result.extend(nodes);
    }

    result
}

/// Lay out an `Absolute` child against containing block `cb`. Anchors to
/// left/top when set, else right/bottom (measuring the child to anchor its far
/// edge), else the containing block's origin (a coarse stand-in for the static
/// position). The child fills `cb` as its available size.
fn layout_absolute<'a>(
    child: &'a UiElement,
    cb: ContainingBlock,
    p: &mut LayoutPass<'a>,
) -> Vec<ComputedElement<'a>> {
    let (cbx, cby, cbw, cbh) = cb.rect;
    // Resolve each side's offset (px + percentage-of-cb) up front.
    let (left, right) = (child.eff_left(cbw), child.eff_right(cbw));
    let (top, bottom) = (child.eff_top(cbh), child.eff_bottom(cbh));

    // Lay out once at the containing-block origin. When a far edge (right/bottom
    // without left/top) is anchored we need the resolved size to place it, but the
    // final box differs only by a translation — so shift the computed subtree
    // rather than laying it out a second time (a second `layout_node` would
    // recompute the whole subtree, and nested far-anchored absolutes would make
    // that O(2^depth); see `translate_nodes` in the in-flow path).
    let mut nodes = layout_node(child, cbx, cby, cbw, cbh, cb.clip, cb, p);
    let (cw, ch) = nodes
        .first()
        .map(|n| (n.width + child.mrg_x(), n.height + child.mrg_y()))
        .unwrap_or((0.0, 0.0));

    let ox = match (left, right) {
        (Some(l), _) => cbx + l,
        (None, Some(r)) => cbx + cbw - r - cw,
        (None, None) => cbx,
    };
    let oy = match (top, bottom) {
        (Some(t), _) => cby + t,
        (None, Some(b)) => cby + cbh - b - ch,
        (None, None) => cby,
    };

    translate_nodes(&mut nodes, ox - cbx, oy - cby);
    nodes
}

/// Leading offset + extra inter-child gap implementing CSS `justify-content`
/// over `free` main-axis pixels shared between `n` children.
fn justify_offsets(j: &JustifyContent, free: f32, n: usize) -> (f32, f32) {
    match j {
        JustifyContent::Start => (0.0, 0.0),
        JustifyContent::Center => (free / 2.0, 0.0),
        JustifyContent::End => (free, 0.0),
        JustifyContent::SpaceBetween => {
            if n > 1 {
                (0.0, free / (n - 1) as f32)
            } else {
                (free / 2.0, 0.0)
            }
        }
        JustifyContent::SpaceAround => {
            let unit = free / n as f32;
            (unit / 2.0, unit)
        }
        JustifyContent::SpaceEvenly => {
            let unit = free / (n + 1) as f32;
            (unit, unit)
        }
    }
}

/// Cross-axis offset for a child of outer cross size `child_cross` within a
/// container whose inner cross extent is `inner_cross` (CSS `align-items`).
/// `Start`/`Stretch` ⇒ 0 (a `Grow` cross size already filled the extent).
fn cross_offset(a: &AlignItems, inner_cross: f32, child_cross: f32) -> f32 {
    let free = (inner_cross - child_cross).max(0.0);
    match a {
        AlignItems::Start | AlignItems::Stretch => 0.0,
        AlignItems::Center => free / 2.0,
        AlignItems::End => free,
    }
}

/// Lay out every root against a `sw`×`sh` surface and return the id of the
/// innermost (top-most) visible element whose drawn, clipped box contains
/// `cursor`. Mirrors [`crate::render`]'s layout so hit-testing matches what was
/// drawn. Used by apps to resolve a click to an element (e.g. a link).
pub fn find_element_at(all: &[UiElement], sw: f32, sh: f32, cursor: (f32, f32)) -> Option<String> {
    let mut roots: Vec<&UiElement> = all.iter().filter(|e| e.parent.is_none()).collect();
    roots.sort_by_key(|e| e.order);

    let full_surface = (0.0, 0.0, sw, sh);
    let cb = surface_cb(sw, sh);
    let mut found: Option<String> = None;
    for root in roots {
        let computed = layout_element(all, root, 0.0, 0.0, sw, sh, full_surface, cb);
        // Parents precede their children in `computed`, so keeping the last
        // containing element yields the innermost match.
        for node in &computed {
            let (cx, cy, cw, ch) = node.clip;
            if cw <= 0.0 || ch <= 0.0 {
                continue;
            }
            if cursor.0 >= cx && cursor.0 < cx + cw && cursor.1 >= cy && cursor.1 < cy + ch {
                found = Some(node.schema.id.clone());
            }
        }
    }
    found
}

/// Lay out every root and, if `cursor` falls on a link [`TextSpan`] of some
/// rich-text element, return that span's `href`. Mirrors [`crate::render`]'s
/// span layout so hit-testing matches the underlined text the user clicked.
/// Returns `None` for clicks on plain text or non-link spans.
pub fn link_at(all: &[UiElement], sw: f32, sh: f32, cursor: (f32, f32)) -> Option<String> {
    let mut roots: Vec<&UiElement> = all.iter().filter(|e| e.parent.is_none()).collect();
    roots.sort_by_key(|e| e.order);

    let full_surface = (0.0, 0.0, sw, sh);
    let cb = surface_cb(sw, sh);
    let mut found: Option<String> = None;
    for root in roots {
        let computed = layout_element(all, root, 0.0, 0.0, sw, sh, full_surface, cb);
        for node in &computed {
            let el = node.schema;
            if el.spans.is_empty() {
                continue;
            }
            let Some(text) = el.text.as_deref() else {
                continue;
            };
            let (cx, cy, cw, ch) = node.clip;
            if cw <= 0.0 || ch <= 0.0 {
                continue;
            }
            if !(cursor.0 >= cx && cursor.0 < cx + cw && cursor.1 >= cy && cursor.1 < cy + ch) {
                continue;
            }

            let inner_w = (node.width - el.pad_x()).max(0.0);
            let base = el.base_font_style();
            let lh = el.line_height_px();
            let text_x = node.x + el.pad_l();
            let text_y0 = node.y + el.pad_t();

            let rel_y = cursor.1 - text_y0;
            if rel_y < 0.0 {
                continue;
            }
            let line_idx = (rel_y / lh).floor() as usize;

            let words = layout_words(text, el.text_size, inner_w, |i| {
                span_style_at(&el.spans, i, base)
            });
            let offsets = line_align_offsets(&words, inner_w, el.text_align);
            for w in &words {
                if w.line != line_idx {
                    continue;
                }
                let ax = offsets.get(w.line).copied().unwrap_or(0.0);
                let wx0 = text_x + ax + w.x;
                let wx1 = wx0 + w.width;
                if cursor.0 >= wx0 && cursor.0 < wx1 {
                    let word_style = span_style_at(&el.spans, w.char_start, base);
                    let off = char_index_at(&w.text, el.text_size, cursor.0 - wx0, word_style);
                    let idx = w.char_start + off;
                    if let Some(href) = span_href_at(&el.spans, idx) {
                        found = Some(href);
                    }
                }
            }
        }
    }
    found
}

/// Walk the layout tree to find the innermost scrollable element containing `cursor`.
#[allow(clippy::too_many_arguments)]
pub fn find_scrollable_at<'a>(
    all: &'a [UiElement],
    el: &'a UiElement,
    cursor: (f32, f32),
    origin_x: f32,
    origin_y: f32,
    avail_w: f32,
    avail_h: f32,
    clip: ClipRect,
    best: &mut Option<(String, bool, bool)>,
) {
    let mut p = LayoutPass::new(all);
    find_scrollable_node(
        el, cursor, origin_x, origin_y, avail_w, avail_h, clip, best, &mut p,
    );
}

#[allow(clippy::too_many_arguments)]
fn find_scrollable_node<'a>(
    el: &'a UiElement,
    cursor: (f32, f32),
    origin_x: f32,
    origin_y: f32,
    avail_w: f32,
    avail_h: f32,
    clip: ClipRect,
    best: &mut Option<(String, bool, bool)>,
    p: &mut LayoutPass<'a>,
) {
    if !el.visible {
        return;
    }

    let x = origin_x + el.mrg_l();
    let y = origin_y + el.mrg_t();
    let own_w = match el.width {
        Size::Fixed(px) => px.max(0.0),
        Size::Percent(f) => (avail_w * f).max(0.0),
        Size::Grow => (avail_w - el.mrg_x()).max(0.0),
        Size::Fit => fit_width(el, avail_w, p),
    };
    let inner_w = (own_w - el.pad_x()).max(0.0);
    let own_h = match el.height {
        Size::Fixed(px) => px.max(0.0),
        Size::Percent(f) => (avail_h * f).max(0.0),
        Size::Grow => (avail_h - el.mrg_y()).max(0.0),
        Size::Fit => fit_height(el, inner_w, p),
    };
    let own_h = cap_height(el, own_h);

    let bounds = (x, y, own_w, own_h);
    let self_clip = intersect_clip(clip, bounds);

    if cursor.0 < x || cursor.0 > x + own_w || cursor.1 < y || cursor.1 > y + own_h {
        return;
    }

    if el.scrollable_x || el.scrollable_y {
        *best = Some((el.id.clone(), el.scrollable_x, el.scrollable_y));
    }

    // Recurse into children.
    let inner_h = (own_h - el.pad_y()).max(0.0);
    let content_clip = intersect_clip(
        self_clip,
        (x + el.pad_l(), y + el.pad_t(), inner_w, inner_h),
    );
    let children = p.children(&el.id);

    for child in children {
        find_scrollable_node(
            child,
            cursor,
            x + el.pad_l(),
            y + el.pad_t(),
            inner_w,
            inner_h,
            content_clip,
            best,
            p,
        );
    }
}

#[cfg(test)]
mod portal_geom {
    use super::*;
    use crate::types::*;

    fn el(id: &str) -> UiElement {
        UiElement {
            id: id.to_string(),
            parent: None,
            order: 0,
            width: Size::Fit,
            height: Size::Fit,
            layout_direction: LayoutDirection::Column,
            justify_content: JustifyContent::Start,
            align_items: AlignItems::Stretch,
            position: Position::Static,
            pos_left: None,
            pos_top: None,
            pos_right: None,
            pos_bottom: None,
            pos_left_pct: None,
            pos_top_pct: None,
            pos_right_pct: None,
            pos_bottom_pct: None,
            gap: 0.0,
            padding: 0.0,
            margin: 0.0,
            padding_sides: None,
            margin_sides: None,
            background_color: (0.0, 0.0, 0.0, 0.0),
            corner_radius: 0.0,
            border_width: 0.0,
            border_color: (0.0, 0.0, 0.0, 0.0),
            text: None,
            text_size: 16.0,
            text_color: (0.0, 0.0, 0.0, 1.0),
            text_wrap: TextWrap::Words,
            text_bold: false,
            text_italic: false,
            spans: Vec::new(),
            text_align: 0.0,
            line_height: 0.0,
            image: None,
            image_src: None,
            is_input: false,
            cursor_pos: 0,
            scrollable_x: false,
            scrollable_y: false,
            scroll_x: 0.0,
            scroll_y: 0.0,
            max_height: None,
            visible: true,
            center_cross: false,
        }
    }

    fn find<'a>(c: &'a [ComputedElement<'a>], id: &str) -> &'a ComputedElement<'a> {
        c.iter().find(|e| e.schema.id == id).expect(id)
    }

    // Reproduces the Wikipedia portal's central region: `.central-textlogo`
    // (position:relative, width 270, margin 4rem auto .5rem) holding the globe
    // (`.central-featured-logo`, position:absolute, top:158 left:35, 200x183)
    // plus the wordmark/slogan text, followed by `.central-featured` (the language
    // ring, position:relative, 54.6rem x 32.5rem, margin 0 auto) whose lang items
    // are position:absolute at percentage insets. Asserts the globe lands centred
    // inside the ring, matching Firefox.
    #[test]
    fn globe_is_centered_in_language_ring() {
        let mut all = Vec::new();

        let mut main = el("main");
        main.width = Size::Fixed(800.0);
        main.align_items = AlignItems::Stretch;
        all.push(main);

        // .central-textlogo — order 0 in main.
        let mut logo = el("logo");
        logo.parent = Some("main".into());
        logo.order = 0;
        logo.position = Position::Relative;
        logo.width = Size::Fixed(270.0);
        logo.center_cross = true; // margin: … auto …
        logo.margin_sides = Some((40.0, 0.0, 5.0, 0.0)); // 4rem top, .5rem bottom
        all.push(logo);

        // wordmark "Wikipedia" (3rem / line 3.3rem) — in-flow child.
        let mut wordmark = el("wordmark");
        wordmark.parent = Some("logo".into());
        wordmark.order = 0;
        wordmark.text = Some("Wikipedia".into());
        wordmark.text_size = 30.0;
        wordmark.line_height = 33.0;
        wordmark.text_align = 0.5;
        all.push(wordmark);

        // slogan "The Free Encyclopedia" (1.5rem) — in-flow child.
        let mut slogan = el("slogan");
        slogan.parent = Some("logo".into());
        slogan.order = 1;
        slogan.text = Some("The Free Encyclopedia".into());
        slogan.text_size = 15.0;
        slogan.text_align = 0.5;
        all.push(slogan);

        // The globe — position:absolute top:158 left:35, 200x183.
        let mut globe = el("globe");
        globe.parent = Some("logo".into());
        globe.order = 2;
        globe.position = Position::Absolute;
        globe.pos_top = Some(158.0);
        globe.pos_left = Some(35.0);
        globe.width = Size::Fixed(200.0);
        globe.height = Size::Fixed(183.0);
        all.push(globe);

        // .central-featured — the ring, order 1 in main.
        let mut ring = el("ring");
        ring.parent = Some("main".into());
        ring.order = 1;
        ring.position = Position::Relative;
        ring.width = Size::Fixed(546.0);
        ring.height = Size::Fixed(325.0);
        ring.center_cross = true; // margin: 0 auto
        all.push(ring);

        // lang1 (English): top:0 right:60%, width 15.6rem.
        let mut lang1 = el("lang1");
        lang1.parent = Some("ring".into());
        lang1.order = 0;
        lang1.position = Position::Absolute;
        lang1.pos_top_pct = Some(0.0);
        lang1.pos_right_pct = Some(0.60);
        lang1.width = Size::Fixed(156.0);
        lang1.height = Size::Fixed(60.0);
        all.push(lang1);

        // lang9: top:80% right:60%.
        let mut lang9 = el("lang9");
        lang9.parent = Some("ring".into());
        lang9.order = 1;
        lang9.position = Position::Absolute;
        lang9.pos_top_pct = Some(0.80);
        lang9.pos_right_pct = Some(0.60);
        lang9.width = Size::Fixed(156.0);
        lang9.height = Size::Fixed(60.0);
        all.push(lang9);

        let root = all[0].clone();
        let full = (0.0, 0.0, 800.0, 2000.0);
        let cb = ContainingBlock {
            rect: full,
            clip: full,
        };
        let computed = layout_element(&all, &root, 0.0, 0.0, 800.0, 2000.0, full, cb);

        let logo_c = find(&computed, "logo");
        let globe_c = find(&computed, "globe");
        let ring_c = find(&computed, "ring");
        let lang1_c = find(&computed, "lang1");
        let lang9_c = find(&computed, "lang9");

        let globe_cy = globe_c.y + globe_c.height / 2.0;
        let ring_cy = ring_c.y + ring_c.height / 2.0;

        eprintln!(
            "logo:  y={:.1} h={:.1}",
            logo_c.y, logo_c.height
        );
        eprintln!(
            "globe: y={:.1} h={:.1} cx={:.1} cy={:.1}",
            globe_c.y,
            globe_c.height,
            globe_c.x + globe_c.width / 2.0,
            globe_cy
        );
        eprintln!(
            "ring:  y={:.1} h={:.1} cx={:.1} cy={:.1}",
            ring_c.y,
            ring_c.height,
            ring_c.x + ring_c.width / 2.0,
            ring_cy
        );
        eprintln!("lang1(EN): y={:.1}  lang9: y={:.1}", lang1_c.y, lang9_c.y);

        // Firefox: the globe sits centred in the ring (English above it, lang9
        // below it). Fails today if the globe rides high over the top languages.
        assert!(
            globe_c.y > lang1_c.y,
            "globe top ({:.1}) should be below English ({:.1})",
            globe_c.y,
            lang1_c.y
        );
        assert!(
            (globe_cy - ring_cy).abs() < 60.0,
            "globe centre ({:.1}) should be near ring centre ({:.1})",
            globe_cy,
            ring_cy
        );
    }

    // CSS `max-height` caps the used height and clips overflowing in-flow content
    // via the engine's existing content-box clip — the language accordion's
    // collapsed (`max-height:0`) and expanded states, without removing the subtree.
    #[test]
    fn max_height_caps_and_clips_without_removing() {
        fn run(cap: Option<f32>) -> (f32, ClipRect) {
            let mut all = Vec::new();

            let mut container = el("container");
            container.width = Size::Fixed(300.0);
            container.height = Size::Fit; // hugs content unless capped
            container.max_height = cap;
            all.push(container);

            // A tall in-flow child (200px) — far taller than a small cap.
            let mut kid = el("kid");
            kid.parent = Some("container".into());
            kid.order = 0;
            kid.height = Size::Fixed(200.0);
            kid.width = Size::Fixed(300.0);
            all.push(kid);

            let root = all[0].clone();
            let full = (0.0, 0.0, 300.0, 2000.0);
            let cb = ContainingBlock {
                rect: full,
                clip: full,
            };
            let computed = layout_element(&all, &root, 0.0, 0.0, 300.0, 2000.0, full, cb);
            let c = find(&computed, "container");
            let k = find(&computed, "kid");
            (c.height, k.clip)
        }

        // Uncapped: the container hugs its 200px child.
        let (h_uncapped, _) = run(None);
        assert!(
            (h_uncapped - 200.0).abs() < 1.0,
            "uncapped container should be ~200 (was {h_uncapped})"
        );

        // Capped at 20: height clamps to 20 and the child is clipped to that box —
        // the child element still exists (not removed), only its paint is bounded.
        let (h_capped, kid_clip) = run(Some(20.0));
        assert!(
            (h_capped - 20.0).abs() < 1.0,
            "capped container should be 20 (was {h_capped})"
        );
        assert!(
            kid_clip.3 <= 20.0 + 0.01,
            "child clip height ({}) should be bounded by the 20px cap",
            kid_clip.3
        );

        // Collapsed accordion (`max-height:0`): height 0 and the child is fully
        // clipped away, yet the node is still laid out (present in `computed`).
        let (h_zero, kid_clip0) = run(Some(0.0));
        assert!(h_zero.abs() < 0.01, "max-height:0 collapses to 0 (was {h_zero})");
        assert!(
            kid_clip0.3.abs() < 0.01,
            "collapsed child clip height ({}) should be 0",
            kid_clip0.3
        );
    }
}
