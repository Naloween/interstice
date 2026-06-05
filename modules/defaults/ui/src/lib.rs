use interstice_sdk::*;

interstice_module!(visibility: Public);

pub use crate::bindings::graphics::*;
pub use crate::bindings::input::*;

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
    None,
    Words,
    Newlines,
}

// ── Tables ───────────────────────────────────────────────────────────────────

#[table(public)]
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
    // Text input
    pub is_input: bool,
    pub cursor_pos: u32,
    // Scroll
    pub scrollable_x: bool,
    pub scrollable_y: bool,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub visible: bool,
}

/// Which element currently has keyboard focus.
#[table(ephemeral)]
pub struct InputFocus {
    #[primary_key]
    pub id: u32,
    pub focused_element: Option<String>,
}

/// Persistent bookkeeping to detect new input events each frame.
#[table]
pub struct UiInputState {
    #[primary_key]
    pub id: u32,
    pub last_input_generation: u64,
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

#[reducer]
pub fn set_focus<Caps>(ctx: ReducerContext<Caps>, id: String)
where
    Caps: CanRead<InputFocus> + CanUpdate<InputFocus>,
{
    if let Some(mut f) = ctx.current.tables.inputfocus().get(0) {
        f.focused_element = Some(id);
        let _ = ctx.current.tables.inputfocus().update(f);
    }
}

#[reducer]
pub fn clear_focus<Caps>(ctx: ReducerContext<Caps>)
where
    Caps: CanRead<InputFocus> + CanUpdate<InputFocus>,
{
    if let Some(mut f) = ctx.current.tables.inputfocus().get(0) {
        f.focused_element = None;
        let _ = ctx.current.tables.inputfocus().update(f);
    }
}

// ── Load ─────────────────────────────────────────────────────────────────────

#[reducer(on = "load")]
pub fn on_load<Caps>(ctx: ReducerContext<Caps>)
where
    Caps: CanInsert<InputFocus> + CanInsert<UiInputState>,
{
    let graphics = ctx.graphics();
    if let Err(err) = graphics.reducers.create_layer(UI_LAYER.to_string(), UI_LAYER_Z, false) {
        ctx.log(&format!("ui: failed to create layer: {err}"));
    }
    let _ = ctx.current.tables.inputfocus().insert(InputFocus { id: 0, focused_element: None });
    let _ = ctx.current.tables.uiinputstate().insert(UiInputState { id: 0, last_input_generation: 0 });
}

// ── Render + input ────────────────────────────────────────────────────────────

#[reducer(on = "graphics.frametick.update")]
pub fn on_frame<Caps>(ctx: ReducerContext<Caps>, _prev: FrameTick, _tick: FrameTick)
where
    Caps: CanRead<UiElement>
        + CanUpdate<UiElement>
        + CanRead<SurfaceInfo>
        + CanRead<InputFocus>
        + CanRead<UiInputState>
        + CanInsert<UiInputState>
        + CanUpdate<UiInputState>
        + CanRead<TextInputBuffer>
        + CanRead<MouseState>,
{
    let surface = ctx.graphics().tables.surfaceinfo().get(0);
    let (sw, sh) = match surface {
        Some(s) if s.width > 0 && s.height > 0 => (s.width as f32, s.height as f32),
        _ => return,
    };

    // Ensure UiInputState row exists — it should be inserted by on_load, but
    // defensively re-create it if somehow missing to prevent infinite repetition.
    if ctx.current.tables.uiinputstate().get(0).is_none() {
        let _ = ctx.current.tables.uiinputstate().insert(UiInputState { id: 0, last_input_generation: 0 });
    }

    // ── Text input ───────────────────────────────────────────────────────────
    let focused_id = ctx
        .current
        .tables
        .inputfocus()
        .get(0)
        .and_then(|f| f.focused_element.clone());

    if let Some(ref fid) = focused_id {
        let buf = ctx.input().tables.textinputbuffer().get(0);
        if let Some(buf) = buf {
            let last_gen = ctx
                .current
                .tables
                .uiinputstate()
                .get(0)
                .map(|s| s.last_input_generation)
                .unwrap_or(0);

            if buf.generation != last_gen && !buf.character.is_empty() {
                // Update generation tracker.
                if let Some(mut state) = ctx.current.tables.uiinputstate().get(0) {
                    state.last_input_generation = buf.generation;
                    let _ = ctx.current.tables.uiinputstate().update(state);
                }

                // Apply character to focused element.
                if let Some(mut el) = ctx.current.tables.uielement().get(fid.clone()) {
                    if el.is_input {
                        let mut text = el.text.clone().unwrap_or_default();
                        if buf.character == "\x08" {
                            // Backspace: remove last char, adjust cursor.
                            if el.cursor_pos > 0 {
                                let byte_pos = char_to_byte_pos(&text, el.cursor_pos as usize - 1);
                                let end = char_to_byte_pos(&text, el.cursor_pos as usize);
                                text.drain(byte_pos..end);
                                el.cursor_pos -= 1;
                            }
                        } else {
                            // Insert character at cursor position.
                            let byte_pos = char_to_byte_pos(&text, el.cursor_pos as usize);
                            text.insert_str(byte_pos, &buf.character);
                            el.cursor_pos += 1;
                        }
                        el.text = Some(text);
                        let _ = ctx.current.tables.uielement().update(el);
                    }
                }
            }
        }
    }

    // ── Scroll ───────────────────────────────────────────────────────────────
    let mouse = ctx.input().tables.mousestate().get(0);
    if let Some(mouse) = mouse {
        let (wx, wy) = mouse.wheel_delta;
        if wx != 0.0 || wy != 0.0 {
            let cursor = mouse.position;
            // Find the innermost scrollable element that contains the cursor.
            let all: Vec<UiElement> = ctx.current.tables.uielement().scan();
            let full_surface = (0.0, 0.0, sw, sh);
            let mut best: Option<(String, bool, bool)> = None;

            for root in all.iter().filter(|e| e.parent.is_none()) {
                find_scrollable_at(&all, root, cursor, 0.0, 0.0, sw, sh, full_surface, &mut best);
            }

            if let Some((sid, sx, sy)) = best {
                if let Some(mut el) = ctx.current.tables.uielement().get(sid) {
                    const SCROLL_SPEED: f32 = 30.0;
                    if sx { el.scroll_x = (el.scroll_x - wx * SCROLL_SPEED).max(0.0); }
                    if sy { el.scroll_y = (el.scroll_y - wy * SCROLL_SPEED).max(0.0); }
                    let _ = ctx.current.tables.uielement().update(el);
                }
            }
        }
    }

    // ── Render ───────────────────────────────────────────────────────────────
    let all: Vec<UiElement> = ctx.current.tables.uielement().scan();
    let mut roots: Vec<&UiElement> = all.iter().filter(|e| e.parent.is_none()).collect();
    roots.sort_by_key(|e| e.order);

    let full_surface = (0.0, 0.0, sw, sh);
    for root in roots {
        let computed = layout_element(&all, root, 0.0, 0.0, sw, sh, full_surface);
        for node in &computed {
            draw_element(&ctx, node, &focused_id);
        }
    }
}

// ── Text metrics ─────────────────────────────────────────────────────────────

fn glyph_advance(size: f32) -> f32 {
    9.0 * (size / 8.0).max(0.125)
}

fn text_line_height(size: f32) -> f32 {
    10.0 * (size / 8.0).max(0.125)
}

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

fn compute_lines(text: &str, size: f32, inner_w: f32, wrap: &TextWrap) -> Vec<String> {
    match wrap {
        TextWrap::None => text.lines().map(|l| l.to_string()).collect(),
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
            if all_lines.is_empty() { all_lines.push(String::new()); }
            all_lines
        }
        TextWrap::Newlines => text.lines().map(|l| l.to_string()).collect(),
    }
}

fn min_text_width(text: &str, size: f32, wrap: &TextWrap) -> f32 {
    let advance = glyph_advance(size);
    match wrap {
        TextWrap::Words => text
            .split_whitespace()
            .map(|w| w.chars().count() as f32 * advance)
            .fold(0.0f32, f32::max),
        TextWrap::None | TextWrap::Newlines => text
            .lines()
            .map(|l| l.chars().count() as f32 * advance)
            .fold(0.0f32, f32::max),
    }
}

fn char_to_byte_pos(s: &str, char_idx: usize) -> usize {
    s.char_indices().nth(char_idx).map(|(i, _)| i).unwrap_or(s.len())
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

fn fit_width(all: &[UiElement], el: &UiElement, avail_w: f32) -> f32 {
    let text_min_w = el.text.as_deref()
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
            let sum: f32 = children.iter()
                .map(|c| child_min_w(all, c, inner_avail) + c.margin * 2.0)
                .sum();
            sum + gap_total
        }
        LayoutDirection::Column => children.iter()
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
            let sum: f32 = children.iter()
                .map(|c| child_resolved_h(all, c, inner_w) + c.margin * 2.0)
                .sum();
            sum + gap_total
        }
        LayoutDirection::Row => children.iter()
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

    let own_w = match el.width {
        Size::Fixed(px) => px.max(0.0),
        Size::Grow => (avail_w - el.margin * 2.0).max(0.0).max(fit_width(all, el, avail_w)),
        Size::Fit => fit_width(all, el, avail_w),
    };
    let inner_w = (own_w - el.padding * 2.0).max(0.0);

    let own_h = match el.height {
        Size::Fixed(px) => px.max(0.0),
        Size::Grow => (avail_h - el.margin * 2.0).max(0.0).max(fit_height(all, el, inner_w)),
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
    let total_gap = if visible_n > 1.0 { el.gap * (visible_n - 1.0) } else { 0.0 };
    let remaining = match el.layout_direction {
        LayoutDirection::Row => (inner_w - fixed_main - total_gap).max(0.0),
        LayoutDirection::Column => (inner_h - fixed_main - total_gap).max(0.0),
    };
    let grow_size = if grow_count > 0 { remaining / grow_count as f32 } else { 0.0 };

    // Apply scroll offset to child origin.
    let scroll_ox = if el.scrollable_x { el.scroll_x } else { 0.0 };
    let scroll_oy = if el.scrollable_y { el.scroll_y } else { 0.0 };

    let mut cursor = 0.0f32;
    let content_x = x + el.padding - scroll_ox;
    let content_y = y + el.padding - scroll_oy;
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

/// Walk the layout tree to find the innermost scrollable element containing `cursor`.
fn find_scrollable_at<'a>(
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
    if !el.visible { return; }

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
    let content_clip = intersect_clip(self_clip, (x + el.padding, y + el.padding, inner_w, inner_h));
    let mut children: Vec<&UiElement> = all
        .iter()
        .filter(|c| c.parent.as_deref() == Some(&el.id) && c.visible)
        .collect();
    children.sort_by_key(|c| c.order);

    for child in children {
        find_scrollable_at(all, child, cursor, x + el.padding, y + el.padding, inner_w, inner_h, content_clip, best);
    }
}

// ── Draw ─────────────────────────────────────────────────────────────────────

fn draw_element<Caps>(ctx: &ReducerContext<Caps>, node: &ComputedElement, focused_id: &Option<String>)
where
    Caps: CanRead<UiElement>,
{
    let (cx, cy, cw, ch) = node.clip;
    if cw <= 0.0 || ch <= 0.0 { return; }

    let el = node.schema;
    let graphics = ctx.graphics();
    let layer = UI_LAYER.to_string();

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
            if text_y + lh <= cy || text_y >= cy + ch { continue; }
            if text_x >= cx + cw { continue; }

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

    // Draw cursor in focused text input.
    if el.is_input {
        let is_focused = focused_id.as_deref() == Some(&el.id);
        if is_focused {
            let text = el.text.as_deref().unwrap_or("");
            let inner_w = (node.width - el.padding * 2.0).max(0.0);
            let advance = glyph_advance(el.text_size);
            let lh = text_line_height(el.text_size);

            // Find cursor screen position (single-line input).
            let cursor_chars = (el.cursor_pos as usize).min(text.chars().count());
            let cursor_x = node.x + el.padding + cursor_chars as f32 * advance;
            let cursor_y = node.y + el.padding;
            let _ = inner_w; // future: handle multi-line cursor

            if cursor_x < cx + cw && cursor_y < cy + ch {
                let _ = graphics.reducers.draw_rect(
                    layer.clone(),
                    Rect { x: cursor_x, y: cursor_y, w: 2.0, h: lh },
                    Color { r: 0.9, g: 0.9, b: 0.9, a: 1.0 },
                    true,
                    0.0,
                    None,
                );
            }
        }

        // Draw input field border highlight when focused.
        let border_color = if focused_id.as_deref() == Some(&el.id) {
            (0.4, 0.6, 1.0, 1.0)
        } else {
            el.border_color
        };
        if el.is_input && focused_id.as_deref() == Some(&el.id) {
            let (br, bg, bb, ba) = border_color;
            let _ = graphics.reducers.draw_rect(
                layer,
                Rect { x: node.x, y: node.y, w: node.width, h: node.height },
                Color { r: br, g: bg, b: bb, a: ba },
                false,
                1.5,
                if el.corner_radius > 0.0 { Some(el.corner_radius) } else { None },
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
