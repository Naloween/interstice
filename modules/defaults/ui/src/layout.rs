use crate::tables::*;
use crate::text::*;
use crate::types::*;
use interstice_sdk::*;

type ClipRect = (f32, f32, f32, f32);

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

    let inner_avail = (avail_w - el.padding * 2.0).max(0.0);
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
                .map(|c| child_min_w(all, c, inner_avail) + c.margin * 2.0)
                .sum();
            sum + gap_total
        }
        LayoutDirection::Column => children
            .iter()
            .map(|c| child_min_w(all, c, inner_avail) + c.margin * 2.0)
            .fold(0.0f32, f32::max),
    };

    (text_min_w.max(children_w) + el.padding * 2.0).max(0.0)
}

fn child_min_w(all: &[UiElement], child: &UiElement, parent_inner_avail: f32) -> f32 {
    match child.width {
        Size::Fixed(px) => px.max(0.0),
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
                .map(|c| child_resolved_h(all, c, inner_w) + c.margin * 2.0)
                .sum();
            sum + gap_total
        }
        LayoutDirection::Row => children
            .iter()
            .map(|c| child_resolved_h(all, c, inner_w) + c.margin * 2.0)
            .fold(0.0f32, f32::max),
    };

    (text_h.max(children_h) + el.padding * 2.0).max(0.0)
}

fn child_resolved_h(all: &[UiElement], child: &UiElement, parent_inner_w: f32) -> f32 {
    let child_outer_w = match child.width {
        Size::Fixed(px) => px.max(0.0),
        Size::Grow | Size::Fit => {
            let min_w = fit_width(all, child, parent_inner_w);
            (parent_inner_w - child.margin * 2.0).max(0.0).max(min_w)
        }
    };
    let child_inner_w = (child_outer_w - child.padding * 2.0).max(0.0);
    match child.height {
        Size::Fixed(px) => px.max(0.0),
        Size::Grow | Size::Fit => fit_height(all, child, child_inner_w),
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
) -> Vec<ComputedElement<'a>> {
    if !el.visible {
        return Vec::new();
    }

    let x = origin_x + el.margin;
    let y = origin_y + el.margin;

    let own_w = match el.width {
        Size::Fixed(px) => px.max(0.0),
        Size::Grow => (avail_w - el.margin * 2.0)
            .max(0.0)
            .max(fit_width(all, el, avail_w)),
        Size::Fit => fit_width(all, el, avail_w),
    };
    let inner_w = (own_w - el.padding * 2.0).max(0.0);

    let own_h = match el.height {
        Size::Fixed(px) => px.max(0.0),
        Size::Grow => (avail_h - el.margin * 2.0)
            .max(0.0)
            .max(fit_height(all, el, inner_w)),
        Size::Fit => fit_height(all, el, inner_w),
    };
    let inner_h = (own_h - el.padding * 2.0).max(0.0);

    let self_clip = intersect_clip(clip, (x, y, own_w, own_h));
    // Content clip also accounts for scroll offset (children are shifted up/left by scroll).
    let content_clip = intersect_clip(
        self_clip,
        (x + el.padding, y + el.padding, inner_w, inner_h),
    );

    let mut children: Vec<&UiElement> = all
        .iter()
        .filter(|c| c.parent.as_deref() == Some(&el.id) && c.visible)
        .collect();
    children.sort_by_key(|c| c.order);

    let (fixed_main, grow_count) =
        children.iter().fold((0.0f32, 0u32), |(acc, gc), child| {
            match el.layout_direction {
                LayoutDirection::Row => match child.width {
                    Size::Grow => (acc, gc + 1),
                    Size::Fixed(px) => (acc + px.max(0.0) + child.margin * 2.0, gc),
                    Size::Fit => {
                        let w = fit_width(all, child, inner_w);
                        (acc + w + child.margin * 2.0, gc)
                    }
                },
                LayoutDirection::Column => match child.height {
                    Size::Grow => (acc, gc + 1),
                    Size::Fixed(px) => (acc + px.max(0.0) + child.margin * 2.0, gc),
                    Size::Fit => {
                        let h = child_resolved_h(all, child, inner_w);
                        (acc + h + child.margin * 2.0, gc)
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

    let mut cursor = 0.0f32;
    let content_x = x + el.padding - scroll_ox;
    let content_y = y + el.padding - scroll_oy;
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

        let child_nodes = layout_element(
            all,
            child,
            child_origin_x,
            child_origin_y,
            child_avail_w,
            child_avail_h,
            content_clip,
        );

        let child_main = match el.layout_direction {
            LayoutDirection::Row => child_nodes
                .first()
                .map(|c| c.width + child.margin * 2.0)
                .unwrap_or(0.0),
            LayoutDirection::Column => child_nodes
                .first()
                .map(|c| c.height + child.margin * 2.0)
                .unwrap_or(0.0),
        };

        cursor += child_main + el.gap;
        result.extend(child_nodes);
    }

    result
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

    let x = origin_x + el.margin;
    let y = origin_y + el.margin;
    let own_w = match el.width {
        Size::Fixed(px) => px.max(0.0),
        Size::Grow => (avail_w - el.margin * 2.0).max(0.0),
        Size::Fit => fit_width(all, el, avail_w),
    };
    let inner_w = (own_w - el.padding * 2.0).max(0.0);
    let own_h = match el.height {
        Size::Fixed(px) => px.max(0.0),
        Size::Grow => (avail_h - el.margin * 2.0).max(0.0),
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
    let inner_h = (own_h - el.padding * 2.0).max(0.0);
    let content_clip = intersect_clip(
        self_clip,
        (x + el.padding, y + el.padding, inner_w, inner_h),
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
            x + el.padding,
            y + el.padding,
            inner_w,
            inner_h,
            content_clip,
            best,
        );
    }
}

pub fn delete_recursive<Caps>(ctx: &ReducerContext<Caps>, id: &str)
where
    Caps: CanRead<UiElement> + CanDelete<UiElement>,
{
    let children: Vec<String> = ctx
        .current
        .tables
        .uielement()
        .scan()
        .into_iter()
        .filter(|e| e.parent.as_deref() == Some(id))
        .map(|e| e.id)
        .collect();
    for child_id in children {
        delete_recursive(ctx, &child_id);
    }
    let _ = ctx.current.tables.uielement().delete(id.to_string());
}
