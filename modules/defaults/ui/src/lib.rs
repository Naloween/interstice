use interstice_sdk::*;

interstice_module!(visibility: Public);

pub use crate::bindings::graphics::*;

const UI_LAYER: &str = "ui";
const UI_LAYER_Z: i32 = 100;

// ── Types ────────────────────────────────────────────────────────────────────

#[interstice_type]
#[derive(Debug, PartialEq)]
pub enum LayoutDirection {
    Row,
    Column,
}

#[interstice_type]
#[derive(Debug, PartialEq)]
pub enum Size {
    Fixed(f32),
    Grow,
    Fit,
}

#[interstice_type]
#[derive(Debug, PartialEq)]
pub enum TextWrap {
    /// No wrapping; text overflows the container.
    None,
    /// Wrap at word boundaries; minimum container width = longest single word.
    Words,
    /// Wrap only at explicit newlines; minimum width = widest explicit line.
    Newlines,
}

// ── Table ────────────────────────────────────────────────────────────────────

#[table]
pub struct UiElement {
    #[primary_key]
    pub id: String,
    pub parent: Option<String>,
    pub order: u32,
    pub width: Size,
    pub height: Size,
    pub layout_direction: LayoutDirection,
    pub gap: f32,
    pub padding: f32,
    pub margin: f32,
    pub background_color: (f32, f32, f32, f32),
    pub corner_radius: f32,
    pub border_width: f32,
    pub border_color: (f32, f32, f32, f32),
    pub text: Option<String>,
    pub text_size: f32,
    pub text_color: (f32, f32, f32, f32),
    pub text_wrap: TextWrap,
    pub visible: bool,
}

// ── Public reducers ──────────────────────────────────────────────────────────

#[reducer]
pub fn create_element<Caps>(ctx: ReducerContext<Caps>, element: UiElement)
where
    Caps: CanInsert<UiElement>,
{
    if let Err(err) = ctx.current.tables.uielement().insert(element) {
        ctx.log(&format!("ui: create_element failed: {err}"));
    }
}

#[reducer]
pub fn update_element<Caps>(ctx: ReducerContext<Caps>, element: UiElement)
where
    Caps: CanUpdate<UiElement>,
{
    if let Err(err) = ctx.current.tables.uielement().update(element) {
        ctx.log(&format!("ui: update_element failed: {err}"));
    }
}

#[reducer]
pub fn delete_element<Caps>(ctx: ReducerContext<Caps>, id: String)
where
    Caps: CanRead<UiElement> + CanDelete<UiElement>,
{
    delete_recursive(&ctx, &id);
}

#[reducer]
pub fn clear_elements<Caps>(ctx: ReducerContext<Caps>)
where
    Caps: CanRead<UiElement> + CanDelete<UiElement>,
{
    for el in ctx.current.tables.uielement().scan() {
        let _ = ctx.current.tables.uielement().delete(el.id);
    }
}

// ── Load ─────────────────────────────────────────────────────────────────────

#[reducer(on = "load")]
pub fn on_load(ctx: ReducerContext) {
    let graphics = ctx.graphics();
    if let Err(err) = graphics.reducers.create_layer(UI_LAYER.to_string(), UI_LAYER_Z, false) {
        ctx.log(&format!("ui: failed to create layer: {err}"));
    }
}

// ── Render ───────────────────────────────────────────────────────────────────

#[reducer(on = "graphics.frametick.update")]
pub fn on_frame<Caps>(ctx: ReducerContext<Caps>, _prev: FrameTick, _tick: FrameTick)
where
    Caps: CanRead<UiElement> + CanRead<SurfaceInfo>,
{
    let surface = ctx.graphics().tables.surfaceinfo().get(0);
    let (sw, sh) = match surface {
        Some(s) if s.width > 0 && s.height > 0 => (s.width as f32, s.height as f32),
        _ => return,
    };

    let all: Vec<UiElement> = ctx.current.tables.uielement().scan();
    let mut roots: Vec<&UiElement> = all.iter().filter(|e| e.parent.is_none()).collect();
    roots.sort_by_key(|e| e.order);

    let full_surface = (0.0, 0.0, sw, sh);
    for root in roots {
        let computed = layout_element(&all, root, 0.0, 0.0, sw, sh, full_surface);
        for node in &computed {
            draw_element(&ctx, node);
        }
    }
}

// ── Text metrics (must match font8x8 tessellation in graphics) ────────────────

fn glyph_advance(size: f32) -> f32 {
    9.0 * (size / 8.0).max(0.125)
}

fn text_line_height(size: f32) -> f32 {
    10.0 * (size / 8.0).max(0.125)
}

/// Wrap one explicit line into sub-lines that fit within `max_w`.
fn word_wrap(line: &str, advance: f32, max_w: f32) -> Vec<String> {
    if max_w <= 0.0 {
        return vec![line.to_string()];
    }
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_w = 0.0f32;

    for word in line.split_whitespace() {
        let word_w = word.chars().count() as f32 * advance;
        let space_w = if current.is_empty() { 0.0 } else { advance };

        if current.is_empty() {
            current.push_str(word);
            current_w = word_w;
        } else if current_w + space_w + word_w <= max_w {
            current.push(' ');
            current.push_str(word);
            current_w += space_w + word_w;
        } else {
            lines.push(current.clone());
            current = word.to_string();
            current_w = word_w;
        }
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

/// Compute the rendered lines for an element's text given a resolved inner width.
fn compute_lines(text: &str, size: f32, inner_w: f32, wrap: &TextWrap) -> Vec<String> {
    match wrap {
        TextWrap::None => {
            // Respect explicit newlines but never word-wrap.
            text.lines().map(|l| l.to_string()).collect()
        }
        TextWrap::Words => {
            let advance = glyph_advance(size);
            let mut all_lines = Vec::new();
            for explicit_line in text.lines() {
                if explicit_line.trim().is_empty() {
                    all_lines.push(String::new());
                } else {
                    all_lines.extend(word_wrap(explicit_line, advance, inner_w));
                }
            }
            if all_lines.is_empty() {
                all_lines.push(String::new());
            }
            all_lines
        }
        TextWrap::Newlines => text.lines().map(|l| l.to_string()).collect(),
    }
}

/// Minimum width that this text can ever occupy (independent of container width).
fn min_text_width(text: &str, size: f32, wrap: &TextWrap) -> f32 {
    let advance = glyph_advance(size);
    match wrap {
        // With word wrap the narrowest valid layout is one word per line.
        TextWrap::Words => text
            .split_whitespace()
            .map(|w| w.chars().count() as f32 * advance)
            .fold(0.0f32, f32::max),
        // Without word wrapping, full line widths are the minimum.
        TextWrap::None | TextWrap::Newlines => text
            .lines()
            .map(|l| l.chars().count() as f32 * advance)
            .fold(0.0f32, f32::max),
    }
}

// ── Layout ───────────────────────────────────────────────────────────────────

type ClipRect = (f32, f32, f32, f32);

struct ComputedElement<'a> {
    schema: &'a UiElement,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    clip: ClipRect,
}

fn intersect_clip(a: ClipRect, b: ClipRect) -> ClipRect {
    let x1 = a.0.max(b.0);
    let y1 = a.1.max(b.1);
    let x2 = (a.0 + a.2).min(b.0 + b.2);
    let y2 = (a.1 + a.3).min(b.1 + b.3);
    (x1, y1, (x2 - x1).max(0.0), (y2 - y1).max(0.0))
}

/// Minimum width for `Fit` sizing (bottom-up, independent of container width).
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
    let gap_total = if visible_n > 1 { el.gap * (visible_n - 1) as f32 } else { 0.0 };

    let children_w = match el.layout_direction {
        LayoutDirection::Row => {
            // Sum of non-Grow children widths + gaps.
            let sum: f32 = children
                .iter()
                .map(|c| child_min_w(all, c, inner_avail) + c.margin * 2.0)
                .sum();
            sum + gap_total
        }
        LayoutDirection::Column => {
            // Max of children widths.
            children
                .iter()
                .map(|c| child_min_w(all, c, inner_avail) + c.margin * 2.0)
                .fold(0.0f32, f32::max)
        }
    };

    (text_min_w.max(children_w) + el.padding * 2.0).max(0.0)
}

/// Minimum width a child contributes to its parent's content minimum.
/// Both Fit and Grow children contribute their content minimum — Grow can expand
/// beyond it, but it cannot shrink below it.
fn child_min_w(all: &[UiElement], child: &UiElement, parent_inner_avail: f32) -> f32 {
    match child.width {
        Size::Fixed(px) => px.max(0.0),
        Size::Grow | Size::Fit => fit_width(all, child, parent_inner_avail),
    }
}

/// Height for `Fit` sizing given a resolved `inner_w` (so text can wrap correctly).
fn fit_height(all: &[UiElement], el: &UiElement, inner_w: f32) -> f32 {
    let text_h = el.text.as_deref().map(|t| {
        let lines = compute_lines(t, el.text_size, inner_w, &el.text_wrap);
        lines.len() as f32 * text_line_height(el.text_size)
    }).unwrap_or(0.0);

    let mut children: Vec<&UiElement> = all
        .iter()
        .filter(|c| c.parent.as_deref() == Some(&el.id) && c.visible)
        .collect();
    children.sort_by_key(|c| c.order);

    let visible_n = children.len();
    let gap_total = if visible_n > 1 { el.gap * (visible_n - 1) as f32 } else { 0.0 };

    let children_h = match el.layout_direction {
        LayoutDirection::Column => {
            // Sum of non-Grow children heights + gaps.
            let sum: f32 = children
                .iter()
                .map(|c| child_resolved_h(all, c, inner_w) + c.margin * 2.0)
                .sum();
            sum + gap_total
        }
        LayoutDirection::Row => {
            // Max of non-Grow children heights.
            children
                .iter()
                .map(|c| child_resolved_h(all, c, inner_w) + c.margin * 2.0)
                .fold(0.0f32, f32::max)
        }
    };

    (text_h.max(children_h) + el.padding * 2.0).max(0.0)
}

/// Resolved minimum height of a child given the parent's `inner_w`.
/// Used when measuring a Column parent's Fit or Grow minimum height.
/// Grow-height children contribute their content minimum for the same reason as Grow-width.
fn child_resolved_h(all: &[UiElement], child: &UiElement, parent_inner_w: f32) -> f32 {
    let child_outer_w = match child.width {
        Size::Fixed(px) => px.max(0.0),
        // Grow/Fit: use max of allocated width and content minimum.
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

fn layout_element<'a>(
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

    // Phase 1: resolve width.
    // Grow fills available space but never shrinks below the minimum content width.
    let own_w = match el.width {
        Size::Fixed(px) => px.max(0.0),
        Size::Grow => (avail_w - el.margin * 2.0).max(0.0).max(fit_width(all, el, avail_w)),
        Size::Fit => fit_width(all, el, avail_w),
    };
    let inner_w = (own_w - el.padding * 2.0).max(0.0);

    // Phase 2: resolve height — uses inner_w so text wrapping is correct.
    // Same minimum-content floor for Grow.
    let own_h = match el.height {
        Size::Fixed(px) => px.max(0.0),
        Size::Grow => (avail_h - el.margin * 2.0).max(0.0).max(fit_height(all, el, inner_w)),
        Size::Fit => fit_height(all, el, inner_w),
    };
    let inner_h = (own_h - el.padding * 2.0).max(0.0);

    let self_clip = intersect_clip(clip, (x, y, own_w, own_h));
    let content_clip =
        intersect_clip(self_clip, (x + el.padding, y + el.padding, inner_w, inner_h));

    let mut children: Vec<&UiElement> = all
        .iter()
        .filter(|c| c.parent.as_deref() == Some(&el.id) && c.visible)
        .collect();
    children.sort_by_key(|c| c.order);

    // Measure fixed/fit children along the main axis to find space for Grow.
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
    let total_gap = if visible_n > 1.0 { el.gap * (visible_n - 1.0) } else { 0.0 };
    let remaining = match el.layout_direction {
        LayoutDirection::Row => (inner_w - fixed_main - total_gap).max(0.0),
        LayoutDirection::Column => (inner_h - fixed_main - total_gap).max(0.0),
    };
    let grow_size = if grow_count > 0 { remaining / grow_count as f32 } else { 0.0 };

    // Place children.
    let mut cursor = 0.0f32;
    let content_x = x + el.padding;
    let content_y = y + el.padding;
    let mut result = Vec::new();
    result.push(ComputedElement { schema: el, x, y, width: own_w, height: own_h, clip: self_clip });

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
            all, child,
            child_origin_x, child_origin_y,
            child_avail_w, child_avail_h,
            content_clip,
        );

        let child_main = match el.layout_direction {
            LayoutDirection::Row => child_nodes.first().map(|c| c.width + child.margin * 2.0).unwrap_or(0.0),
            LayoutDirection::Column => child_nodes.first().map(|c| c.height + child.margin * 2.0).unwrap_or(0.0),
        };

        cursor += child_main + el.gap;
        result.extend(child_nodes);
    }

    result
}

// ── Draw ─────────────────────────────────────────────────────────────────────

fn draw_element<Caps>(ctx: &ReducerContext<Caps>, node: &ComputedElement)
where
    Caps: CanRead<UiElement>,
{
    let (cx, cy, cw, ch) = node.clip;
    if cw <= 0.0 || ch <= 0.0 {
        return;
    }

    let el = node.schema;
    let graphics = ctx.graphics();
    let layer = UI_LAYER.to_string();

    // Draw background and border at the element's full natural size.
    // Trimming the rect dimensions to the clip region would distort corner radii
    // (a 326px card clipped to 68px would render with new corners at the cut edge).
    // GPU hardware clips at the window boundary via NDC, giving a clean flat cut.
    if node.width > 0.0 && node.height > 0.0 {
        let (r, g, b, a) = el.background_color;
        if a > 0.0 {
            let _ = graphics.reducers.draw_rect(
                layer.clone(),
                Rect { x: node.x, y: node.y, w: node.width, h: node.height },
                Color { r, g, b, a },
                true,
                0.0,
                if el.corner_radius > 0.0 { Some(el.corner_radius) } else { None },
            );
        }

        if el.border_width > 0.0 {
            let (br, bg, bb, ba) = el.border_color;
            let _ = graphics.reducers.draw_rect(
                layer.clone(),
                Rect { x: node.x, y: node.y, w: node.width, h: node.height },
                Color { r: br, g: bg, b: bb, a: ba },
                false,
                el.border_width,
                if el.corner_radius > 0.0 { Some(el.corner_radius) } else { None },
            );
        }
    }

    if let Some(text) = &el.text {
        let inner_w = (node.width - el.padding * 2.0).max(0.0);
        let lines = compute_lines(text, el.text_size, inner_w, &el.text_wrap);
        let lh = text_line_height(el.text_size);
        let (tr, tg, tb, ta) = el.text_color;
        let text_x = node.x + el.padding;

        for (i, line) in lines.iter().enumerate() {
            let text_y = node.y + el.padding + i as f32 * lh;

            // Skip lines that are entirely outside the clip rect.
            if text_y + lh <= cy || text_y >= cy + ch {
                continue;
            }
            if text_x >= cx + cw {
                continue;
            }

            let _ = graphics.reducers.draw_text(
                layer.clone(),
                line.clone(),
                Vec2 { x: text_x, y: text_y },
                el.text_size,
                Color { r: tr, g: tg, b: tb, a: ta },
                None,
            );
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn delete_recursive<Caps>(ctx: &ReducerContext<Caps>, id: &str)
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
