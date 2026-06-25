use crate::text::*;
use crate::types::*;

type ClipRect = (f32, f32, f32, f32);

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
/// left/top, fall back to the negated right/bottom, else 0.
fn relative_shift(el: &UiElement) -> (f32, f32) {
    let dx = el
        .pos_left
        .or_else(|| el.pos_right.map(|r| -r))
        .unwrap_or(0.0);
    let dy = el
        .pos_top
        .or_else(|| el.pos_bottom.map(|b| -b))
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

fn intersect_clip(a: ClipRect, b: ClipRect) -> ClipRect {
    let x1 = a.0.max(b.0);
    let y1 = a.1.max(b.1);
    let x2 = (a.0 + a.2).min(b.0 + b.2);
    let y2 = (a.1 + a.3).min(b.1 + b.3);
    (x1, y1, (x2 - x1).max(0.0), (y2 - y1).max(0.0))
}

fn fit_width(all: &[UiElement], el: &UiElement, avail_w: f32) -> f32 {
    let text_min_w = el
        .text
        .as_deref()
        .map(|t| min_text_width(t, el.text_size, &el.text_wrap))
        .unwrap_or(0.0);

    let mut children: Vec<&UiElement> = all
        .iter()
        .filter(|c| c.parent.as_deref() == Some(&el.id) && c.visible)
        .collect();
    children.sort_by_key(|c| c.order);

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
                .map(|c| child_min_w(all, c, inner_avail) + c.mrg_x())
                .sum();
            sum + gap_total
        }
        LayoutDirection::Column => children
            .iter()
            .map(|c| child_min_w(all, c, inner_avail) + c.mrg_x())
            .fold(0.0f32, f32::max),
    };

    (text_min_w.max(children_w) + el.pad_x()).max(0.0)
}

fn child_min_w(all: &[UiElement], child: &UiElement, parent_inner_avail: f32) -> f32 {
    match child.width {
        Size::Fixed(px) => px.max(0.0),
        Size::Percent(f) => (parent_inner_avail * f).max(0.0),
        Size::Grow | Size::Fit => fit_width(all, child, parent_inner_avail),
    }
}

fn fit_height(all: &[UiElement], el: &UiElement, inner_w: f32) -> f32 {
    let text_h = el
        .text
        .as_deref()
        .map(|t| {
            let lines = compute_lines(t, el.text_size, inner_w, &el.text_wrap);
            lines.len() as f32 * text_line_height(el.text_size)
        })
        .unwrap_or(0.0);

    let mut children: Vec<&UiElement> = all
        .iter()
        .filter(|c| c.parent.as_deref() == Some(&el.id) && c.visible)
        .collect();
    children.sort_by_key(|c| c.order);

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
                .map(|c| child_resolved_h(all, c, inner_w) + c.mrg_y())
                .sum();
            sum + gap_total
        }
        LayoutDirection::Row => children
            .iter()
            .map(|c| child_resolved_h(all, c, inner_w) + c.mrg_y())
            .fold(0.0f32, f32::max),
    };

    (text_h.max(children_h) + el.pad_y()).max(0.0)
}

fn child_resolved_h(all: &[UiElement], child: &UiElement, parent_inner_w: f32) -> f32 {
    let child_outer_w = match child.width {
        Size::Fixed(px) => px.max(0.0),
        Size::Percent(f) => (parent_inner_w * f).max(0.0),
        Size::Grow | Size::Fit => {
            let min_w = fit_width(all, child, parent_inner_w);
            (parent_inner_w - child.mrg_x()).max(0.0).max(min_w)
        }
    };
    let child_inner_w = (child_outer_w - child.pad_x()).max(0.0);
    match child.height {
        Size::Fixed(px) => px.max(0.0),
        // A percent height against an indefinite (content-sized) parent resolves
        // to auto in CSS — treat it like Fit/Grow here.
        Size::Percent(_) | Size::Grow | Size::Fit => fit_height(all, child, child_inner_w),
    }
}

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
    if !el.visible {
        return Vec::new();
    }

    // `Relative` shifts this box (and its subtree) without affecting siblings:
    // the parent still advances by the element's flow size, computed from width/
    // height, not position.
    let (rel_dx, rel_dy) = if el.position == Position::Relative {
        relative_shift(el)
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
            .max(fit_width(all, el, avail_w)),
        Size::Fit => fit_width(all, el, avail_w),
    };
    let inner_w = (own_w - el.pad_x()).max(0.0);

    let own_h = match el.height {
        Size::Fixed(px) => px.max(0.0),
        Size::Percent(f) => (avail_h * f).max(0.0),
        Size::Grow => (avail_h - el.mrg_y())
            .max(0.0)
            .max(fit_height(all, el, inner_w)),
        Size::Fit => fit_height(all, el, inner_w),
    };
    let inner_h = (own_h - el.pad_y()).max(0.0);

    let self_clip = intersect_clip(clip, (x, y, own_w, own_h));
    // Content clip also accounts for scroll offset (children are shifted up/left by scroll).
    let content_clip = intersect_clip(
        self_clip,
        (x + el.pad_l(), y + el.pad_t(), inner_w, inner_h),
    );

    let mut all_children: Vec<&UiElement> = all
        .iter()
        .filter(|c| c.parent.as_deref() == Some(&el.id) && c.visible)
        .collect();
    all_children.sort_by_key(|c| c.order);
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
                        let w = fit_width(all, child, inner_w);
                        (acc + w + child.mrg_x(), gc)
                    }
                },
                LayoutDirection::Column => match child.height {
                    Size::Grow => (acc, gc + 1),
                    Size::Fixed(px) => (acc + px.max(0.0) + child.mrg_y(), gc),
                    Size::Percent(f) => (acc + (inner_h * f).max(0.0) + child.mrg_y(), gc),
                    Size::Fit => {
                        let h = child_resolved_h(all, child, inner_w);
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

    let mut cursor = lead;
    let content_x = x + el.pad_l() - scroll_ox;
    let content_y = y + el.pad_t() - scroll_oy;

    // Containing block handed to descendants: a positioned element (relative or
    // absolute) becomes the anchor for its absolute descendants; otherwise the
    // inherited containing block passes straight through.
    let child_cb = if el.position == Position::Static {
        cb
    } else {
        ContainingBlock {
            rect: (content_x, content_y, inner_w, inner_h),
            clip: content_clip,
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
        let (child_avail_w, child_avail_h) = match el.layout_direction {
            LayoutDirection::Row => (grow_size, inner_h),
            LayoutDirection::Column => (inner_w, grow_size),
        };
        let (child_origin_x, child_origin_y) = match el.layout_direction {
            LayoutDirection::Row => (content_x + cursor, content_y),
            LayoutDirection::Column => (content_x, content_y + cursor),
        };

        // Lay out once at the cross-start. For `Center`/`End` we then need the
        // child's resolved cross size to compute its offset and re-lay it out at
        // the final origin so its clip is computed correctly (Start/Stretch keep
        // the single-pass path).
        let mut child_nodes = layout_element(
            all,
            child,
            child_origin_x,
            child_origin_y,
            child_avail_w,
            child_avail_h,
            content_clip,
            child_cb,
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
        let cross_off = cross_offset(&el.align_items, inner_cross, child_cross);
        if cross_off > 0.0 {
            let (ox, oy) = match el.layout_direction {
                LayoutDirection::Row => (child_origin_x, child_origin_y + cross_off),
                LayoutDirection::Column => (child_origin_x + cross_off, child_origin_y),
            };
            child_nodes = layout_element(
                all,
                child,
                ox,
                oy,
                child_avail_w,
                child_avail_h,
                content_clip,
                child_cb,
            );
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
        let nodes = layout_absolute(all, child, child_cb);
        result.extend(nodes);
    }

    result
}

/// Lay out an `Absolute` child against containing block `cb`. Anchors to
/// left/top when set, else right/bottom (measuring the child to anchor its far
/// edge), else the containing block's origin (a coarse stand-in for the static
/// position). The child fills `cb` as its available size.
fn layout_absolute<'a>(
    all: &'a [UiElement],
    child: &'a UiElement,
    cb: ContainingBlock,
) -> Vec<ComputedElement<'a>> {
    let (cbx, cby, cbw, cbh) = cb.rect;
    let need_w = child.pos_left.is_none() && child.pos_right.is_some();
    let need_h = child.pos_top.is_none() && child.pos_bottom.is_some();

    // Measure first only when anchoring a far edge needs the resolved size.
    let (mut cw, mut ch) = (0.0f32, 0.0f32);
    if need_w || need_h {
        let probe = layout_element(all, child, cbx, cby, cbw, cbh, cb.clip, cb);
        if let Some(n) = probe.first() {
            cw = n.width + child.mrg_x();
            ch = n.height + child.mrg_y();
        }
    }

    let ox = match (child.pos_left, child.pos_right) {
        (Some(l), _) => cbx + l,
        (None, Some(r)) => cbx + cbw - r - cw,
        (None, None) => cbx,
    };
    let oy = match (child.pos_top, child.pos_bottom) {
        (Some(t), _) => cby + t,
        (None, Some(b)) => cby + cbh - b - ch,
        (None, None) => cby,
    };

    layout_element(all, child, ox, oy, cbw, cbh, cb.clip, cb)
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
            let lh = text_line_height(el.text_size);
            let advance = glyph_advance(el.text_size);
            let text_x = node.x + el.pad_l();
            let text_y0 = node.y + el.pad_t();

            let rel_y = cursor.1 - text_y0;
            if rel_y < 0.0 {
                continue;
            }
            let line_idx = (rel_y / lh).floor() as usize;

            let words = layout_words(text, el.text_size, inner_w);
            let offsets = line_align_offsets(&words, inner_w, advance, el.text_align);
            for w in &words {
                if w.line != line_idx {
                    continue;
                }
                let ax = offsets.get(w.line).copied().unwrap_or(0.0);
                let wlen = w.text.chars().count();
                let wx0 = text_x + ax + w.x;
                let wx1 = wx0 + wlen as f32 * advance;
                if cursor.0 >= wx0 && cursor.0 < wx1 {
                    let off = ((cursor.0 - wx0) / advance).floor() as usize;
                    let idx = w.char_start + off.min(wlen.saturating_sub(1));
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
    if !el.visible {
        return;
    }

    let x = origin_x + el.mrg_l();
    let y = origin_y + el.mrg_t();
    let own_w = match el.width {
        Size::Fixed(px) => px.max(0.0),
        Size::Percent(f) => (avail_w * f).max(0.0),
        Size::Grow => (avail_w - el.mrg_x()).max(0.0),
        Size::Fit => fit_width(all, el, avail_w),
    };
    let inner_w = (own_w - el.pad_x()).max(0.0);
    let own_h = match el.height {
        Size::Fixed(px) => px.max(0.0),
        Size::Percent(f) => (avail_h * f).max(0.0),
        Size::Grow => (avail_h - el.mrg_y()).max(0.0),
        Size::Fit => fit_height(all, el, inner_w),
    };

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
    let mut children: Vec<&UiElement> = all
        .iter()
        .filter(|c| c.parent.as_deref() == Some(&el.id) && c.visible)
        .collect();
    children.sort_by_key(|c| c.order);

    for child in children {
        find_scrollable_at(
            all,
            child,
            cursor,
            x + el.pad_l(),
            y + el.pad_t(),
            inner_w,
            inner_h,
            content_clip,
            best,
        );
    }
}
