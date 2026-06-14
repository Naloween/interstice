use crate::UI_CURSOR_LAYER;
use crate::UI_LAYER;
use crate::bindings::graphics::*;
use crate::bindings::input::*;
use crate::layout::*;
use crate::tables::*;
use crate::text::*;
use interstice_sdk::*;

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
        let _ = ctx.current.tables.uiinputstate().insert(UiInputState {
            id: 0,
            last_input_generation: 0,
        });
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
                find_scrollable_at(
                    &all,
                    root,
                    cursor,
                    0.0,
                    0.0,
                    sw,
                    sh,
                    full_surface,
                    &mut best,
                );
            }

            if let Some((sid, sx, sy)) = best {
                if let Some(mut el) = ctx.current.tables.uielement().get(sid) {
                    const SCROLL_SPEED: f32 = 30.0;
                    if sx {
                        el.scroll_x = (el.scroll_x - wx * SCROLL_SPEED).max(0.0);
                    }
                    if sy {
                        el.scroll_y = (el.scroll_y - wy * SCROLL_SPEED).max(0.0);
                    }
                    let _ = ctx.current.tables.uielement().update(el);
                }
            }
        }
    }

    // ── Render ───────────────────────────────────────────────────────────────
    let focused_id = ctx
        .current
        .tables
        .inputfocus()
        .get(0)
        .and_then(|f| f.focused_element.clone());
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

    draw_cursor(&ctx, sw, sh);
}

/// Draw the OS mouse cursor on a dedicated top layer, so every module that uses
/// the UI gets a consistent cursor without rendering one itself.
fn draw_cursor<Caps>(ctx: &ReducerContext<Caps>, sw: f32, sh: f32)
where
    Caps: CanRead<MouseState>,
{
    let mouse = ctx.input().tables.mousestate().get(0);
    let (mx, my) = mouse.map(|m| m.position).unwrap_or((sw * 0.5, sh * 0.5));
    let graphics = ctx.graphics();
    let _ = graphics.reducers.draw_circle(
        UI_CURSOR_LAYER.to_string(),
        Vec2 { x: mx, y: my },
        6.0,
        Color {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.9,
        },
        true,
        0.0,
    );
    let _ = graphics.reducers.draw_circle(
        UI_CURSOR_LAYER.to_string(),
        Vec2 { x: mx, y: my },
        6.0,
        Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.6,
        },
        false,
        1.5,
    );
}

fn draw_element<Caps>(
    ctx: &ReducerContext<Caps>,
    node: &ComputedElement,
    focused_id: &Option<String>,
) where
    Caps: CanRead<UiElement>,
{
    let (cx, cy, cw, ch) = node.clip;
    if cw <= 0.0 || ch <= 0.0 {
        return;
    }

    let el = node.schema;
    let graphics = ctx.graphics();
    let layer = UI_LAYER.to_string();

    if node.width > 0.0 && node.height > 0.0 {
        let (r, g, b, a) = el.background_color;
        if a > 0.0 {
            let _ = graphics.reducers.draw_rect(
                layer.clone(),
                Rect {
                    x: node.x,
                    y: node.y,
                    w: node.width,
                    h: node.height,
                },
                Color { r, g, b, a },
                true,
                0.0,
                if el.corner_radius > 0.0 {
                    Some(el.corner_radius)
                } else {
                    None
                },
            );
        }

        if el.border_width > 0.0 {
            let (br, bg, bb, ba) = el.border_color;
            let _ = graphics.reducers.draw_rect(
                layer.clone(),
                Rect {
                    x: node.x,
                    y: node.y,
                    w: node.width,
                    h: node.height,
                },
                Color {
                    r: br,
                    g: bg,
                    b: bb,
                    a: ba,
                },
                false,
                el.border_width,
                if el.corner_radius > 0.0 {
                    Some(el.corner_radius)
                } else {
                    None
                },
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
            if text_y + lh <= cy || text_y >= cy + ch {
                continue;
            }
            if text_x >= cx + cw {
                continue;
            }

            let _ = graphics.reducers.draw_text(
                layer.clone(),
                line.clone(),
                Vec2 {
                    x: text_x,
                    y: text_y,
                },
                el.text_size,
                Color {
                    r: tr,
                    g: tg,
                    b: tb,
                    a: ta,
                },
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
                    Rect {
                        x: cursor_x,
                        y: cursor_y,
                        w: 2.0,
                        h: lh,
                    },
                    Color {
                        r: 0.9,
                        g: 0.9,
                        b: 0.9,
                        a: 1.0,
                    },
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
                Rect {
                    x: node.x,
                    y: node.y,
                    w: node.width,
                    h: node.height,
                },
                Color {
                    r: br,
                    g: bg,
                    b: bb,
                    a: ba,
                },
                false,
                1.5,
                if el.corner_radius > 0.0 {
                    Some(el.corner_radius)
                } else {
                    None
                },
            );
        }
    }
}
