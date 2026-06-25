use crate::layout::*;
use crate::text::*;
use crate::types::*;

pub type Rgba = (f32, f32, f32, f32);

/// Backend the UI engine draws through. A consuming module implements this by
/// forwarding to its own graphics binding (one instance is emitted by
/// [`crate::ui_subsystem`]); the engine itself stays binding-agnostic so the
/// same layout/draw code is shared by every module.
pub trait DrawTarget {
    /// Filled (`filled = true`) or stroked rounded rectangle.
    fn rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: Rgba,
        filled: bool,
        stroke_width: f32,
        corner_radius: Option<f32>,
    );
    /// A single line of text with its top-left at `(x, y)`.
    fn text(&mut self, content: &str, x: f32, y: f32, size: f32, color: Rgba);
    /// Filled or stroked circle centred at `(x, y)`.
    fn circle(&mut self, x: f32, y: f32, r: f32, color: Rgba, filled: bool, stroke_width: f32);
    /// Draw the texture `local_id` into the box `(x, y, w, h)`.
    /// Draw the sub-region `[u0,u1]×[v0,v1]` (normalised UV) of `local_id` into
    /// the destination rect. Callers clipping a partially-visible image pass the
    /// cropped destination rect plus the matching UV window; an uncropped image
    /// uses `0,0,1,1`.
    #[allow(clippy::too_many_arguments)]
    fn image(
        &mut self,
        local_id: &str,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        u0: f32,
        v0: f32,
        u1: f32,
        v1: f32,
    );
}

/// Lay out every root in `all` against a `sw`x`sh` surface and draw it into
/// `target`. `focused` is the id of the element holding keyboard focus (drives
/// the text caret + input highlight). Does NOT draw a mouse cursor — that is the
/// compositor's responsibility (see [`draw_cursor`]).
pub fn render<T: DrawTarget>(all: &[UiElement], sw: f32, sh: f32, focused: Option<&str>, target: &mut T) {
    let mut roots: Vec<&UiElement> = all.iter().filter(|e| e.parent.is_none()).collect();
    roots.sort_by_key(|e| e.order);

    let full_surface = (0.0, 0.0, sw, sh);
    let cb = surface_cb(sw, sh);
    for root in roots {
        let computed = layout_element(all, root, 0.0, 0.0, sw, sh, full_surface, cb);
        for node in &computed {
            draw_element(node, focused, target);
        }
    }
}

/// Draw a software mouse cursor at `(mx, my)`. Only the surface owner that acts
/// as the compositor (a standalone app's root, or the desktop) should call this,
/// so the cursor is never trapped inside a composited child surface.
pub fn draw_cursor<T: DrawTarget>(target: &mut T, mx: f32, my: f32) {
    target.circle(mx, my, 6.0, (1.0, 1.0, 1.0, 0.9), true, 0.0);
    target.circle(mx, my, 6.0, (0.0, 0.0, 0.0, 0.6), false, 1.5);
}

/// Intersect a filled axis-aligned rect with `clip`. Returns the visible slice
/// `(x, y, w, h)`, or `None` if nothing remains. The backend has no scissor, so
/// callers must hand it pre-clipped geometry — otherwise a box scrolled partway
/// under a sibling (e.g. a toolbar) would paint over it.
fn clip_filled(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    clip: (f32, f32, f32, f32),
) -> Option<(f32, f32, f32, f32)> {
    let (cx, cy, cw, ch) = clip;
    let x0 = x.max(cx);
    let y0 = y.max(cy);
    let x1 = (x + w).min(cx + cw);
    let y1 = (y + h).min(cy + ch);
    if x1 > x0 && y1 > y0 {
        Some((x0, y0, x1 - x0, y1 - y0))
    } else {
        None
    }
}

/// Whether the rect lies wholly within `clip` (small epsilon for fp slop).
/// Strokes can't be clipped geometrically without a scissor, so a partially
/// clipped border/highlight is dropped rather than drawn along the clip edge.
fn fully_inside(x: f32, y: f32, w: f32, h: f32, clip: (f32, f32, f32, f32)) -> bool {
    const EPS: f32 = 0.5;
    let (cx, cy, cw, ch) = clip;
    x >= cx - EPS && y >= cy - EPS && x + w <= cx + cw + EPS && y + h <= cy + ch + EPS
}

fn draw_element<T: DrawTarget>(node: &ComputedElement, focused_id: Option<&str>, target: &mut T) {
    let (cx, cy, cw, ch) = node.clip;
    if cw <= 0.0 || ch <= 0.0 {
        return;
    }

    let el = node.schema;
    let corner = if el.corner_radius > 0.0 {
        Some(el.corner_radius)
    } else {
        None
    };

    if node.width > 0.0 && node.height > 0.0 {
        let (_, _, _, a) = el.background_color;
        if a > 0.0 {
            // Clip the fill to the clip rect (no GPU scissor). Rounded corners
            // only survive an unclipped box — a clipped edge has no corner.
            if let Some((rx, ry, rw, rh)) =
                clip_filled(node.x, node.y, node.width, node.height, node.clip)
            {
                let cr = if fully_inside(node.x, node.y, node.width, node.height, node.clip) {
                    corner
                } else {
                    None
                };
                target.rect(rx, ry, rw, rh, el.background_color, true, 0.0, cr);
            }
        }

        // A stroke can't be clipped without drawing along the clip edge, so only
        // paint the border when the whole box is visible.
        if el.border_width > 0.0 && fully_inside(node.x, node.y, node.width, node.height, node.clip)
        {
            target.rect(
                node.x,
                node.y,
                node.width,
                node.height,
                el.border_color,
                false,
                el.border_width,
                corner,
            );
        }
    }

    // Image fills the content box (inside padding). Drawn after the background
    // so it sits on top of any backdrop, below text.
    if let Some(img) = &el.image {
        if !img.is_empty() {
            let ix = node.x + el.pad_l();
            let iy = node.y + el.pad_t();
            let iw = (node.width - el.pad_x()).max(0.0);
            let ih = (node.height - el.pad_y()).max(0.0);
            if iw > 0.0 && ih > 0.0 {
                // Crop the image to the clip rect (e.g. when scrolled partly under
                // a toolbar): intersect the image box with the clip, then map the
                // visible window back to UV space so only that slice is sampled.
                // Without this the whole texture draws into a shifted rect and
                // spills past the viewport.
                let vx0 = ix.max(cx);
                let vy0 = iy.max(cy);
                let vx1 = (ix + iw).min(cx + cw);
                let vy1 = (iy + ih).min(cy + ch);
                if vx1 > vx0 && vy1 > vy0 {
                    let u0 = (vx0 - ix) / iw;
                    let v0 = (vy0 - iy) / ih;
                    let u1 = (vx1 - ix) / iw;
                    let v1 = (vy1 - iy) / ih;
                    target.image(img, vx0, vy0, vx1 - vx0, vy1 - vy0, u0, v0, u1, v1);
                }
            }
        }
    }

    // Guard on a positive size: the backend rejects non-positive text sizes, and
    // CSS legitimately yields `font-size:0` (e.g. to collapse inline-block gaps).
    if let (Some(text), true) = (&el.text, el.text_size > 0.0) {
        let inner_w = (node.width - el.pad_x()).max(0.0);
        let lh = text_line_height(el.text_size);
        let text_x = node.x + el.pad_l();
        let text_y0 = node.y + el.pad_t();

        let advance = glyph_advance(el.text_size);
        if el.spans.is_empty() {
            // Plain single-colour text: wrap the whole string and draw line-by-line.
            let lines = compute_lines(text, el.text_size, inner_w, &el.text_wrap);
            for (i, line) in lines.iter().enumerate() {
                let text_y = text_y0 + i as f32 * lh;
                // Cull a line that starts above the clip (would spill over a
                // toolbar) as well as one fully below it — glyph rows can't be
                // partially scissored.
                if text_y < cy || text_y >= cy + ch {
                    continue;
                }
                let line_w = line.chars().count() as f32 * advance;
                let ax = align_offset(inner_w, line_w, el.text_align);
                if text_x + ax >= cx + cw {
                    continue;
                }
                target.text(line, text_x + ax, text_y, el.text_size, el.text_color);
            }
        } else {
            // Rich text: lay out words (preserving char offsets) and draw each as
            // colour-uniform sub-runs, underlining link spans for affordance.
            let words = layout_words(text, el.text_size, inner_w);
            let line_offsets = line_align_offsets(&words, inner_w, advance, el.text_align);
            for w in &words {
                let wy = text_y0 + w.line as f32 * lh;
                if wy < cy || wy >= cy + ch {
                    continue;
                }
                let ax = line_offsets.get(w.line).copied().unwrap_or(0.0);
                let wchars: Vec<char> = w.text.chars().collect();
                let mut k = 0;
                while k < wchars.len() {
                    let color = span_color_at(&el.spans, w.char_start + k, el.text_color);
                    let is_link = span_href_at(&el.spans, w.char_start + k).is_some();
                    let mut j = k + 1;
                    while j < wchars.len()
                        && span_color_at(&el.spans, w.char_start + j, el.text_color) == color
                        && span_href_at(&el.spans, w.char_start + j).is_some() == is_link
                    {
                        j += 1;
                    }
                    let seg_x = text_x + ax + w.x + k as f32 * advance;
                    if seg_x < cx + cw {
                        let seg: String = wchars[k..j].iter().collect();
                        target.text(&seg, seg_x, wy, el.text_size, color);
                        if is_link {
                            let seg_w = (j - k) as f32 * advance;
                            if let Some((rx, ry, rw, rh)) =
                                clip_filled(seg_x, wy + lh - 1.5, seg_w, 1.0, node.clip)
                            {
                                target.rect(rx, ry, rw, rh, color, true, 0.0, None);
                            }
                        }
                    }
                    k = j;
                }
            }
        }
    }

    // Text input caret + focus highlight.
    if el.is_input {
        let is_focused = focused_id == Some(el.id.as_str());
        if is_focused {
            let text = el.text.as_deref().unwrap_or("");
            let advance = glyph_advance(el.text_size);
            let lh = text_line_height(el.text_size);

            // Cursor screen position (single-line input).
            let cursor_chars = (el.cursor_pos as usize).min(text.chars().count());
            let cursor_x = node.x + el.pad_l() + cursor_chars as f32 * advance;
            let cursor_y = node.y + el.pad_t();

            if cursor_x < cx + cw && cursor_y < cy + ch {
                target.rect(cursor_x, cursor_y, 2.0, lh, (0.9, 0.9, 0.9, 1.0), true, 0.0, None);
            }

            // Focused input border highlight.
            target.rect(
                node.x,
                node.y,
                node.width,
                node.height,
                (0.4, 0.6, 1.0, 1.0),
                false,
                1.5,
                corner,
            );
        }
    }
}
