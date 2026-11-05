//! The Interstice brand, in code (brand/BRAND.md). Every example takes its
//! colours, sizes and brand textures from here; a value missing here is added
//! here, never hard-coded in a module.
//!
//! Colours are straight-alpha RGBA tuples in 0..1, the form `UiElement` and
//! the graphics bindings use. Textures are tightly packed RGBA8, straight
//! alpha, ready for `create_texture` + `draw_image` (the 2D renderer has no
//! blur, so glows are baked into textures once).

use crate::text::{FontStyle, char_advance};

pub type Rgba = (f32, f32, f32, f32);

/// `0xRRGGBB` to an opaque colour.
pub const fn hex(rgb: u32) -> Rgba {
    (
        ((rgb >> 16) & 0xff) as f32 / 255.0,
        ((rgb >> 8) & 0xff) as f32 / 255.0,
        (rgb & 0xff) as f32 / 255.0,
        1.0,
    )
}

/// `c` at alpha `a`.
pub const fn alpha(c: Rgba, a: f32) -> Rgba {
    (c.0, c.1, c.2, a)
}

/// Linear mix from `a` (t = 0) to `b` (t = 1), alpha included.
pub fn mix(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let t = t.clamp(0.0, 1.0);
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t, a.3 + (b.3 - a.3) * t)
}

// ── Colour tokens (BRAND.md §3) ───────────────────────────────────────────────

/// Wells: inputs, terminals, the game arena.
pub const INK: Rgba = hex(0x0F1012);
/// Desktop wallpaper, page background behind panels.
pub const GROUND: Rgba = hex(0x151618);
/// Windows, panels, the top bar: the main surface.
pub const CHARCOAL: Rgba = hex(0x181B1F);
/// Title bars, tiles, buttons, menus, hovered rows.
pub const RAISED: Rgba = hex(0x212429);
/// Hairlines, borders, dividers.
pub const LINE: Rgba = hex(0x2C2F35);
/// Primary text.
pub const IVORY: Rgba = hex(0xE8E9E6);
/// Secondary text that must be read at small sizes.
pub const MUTED: Rgba = hex(0x8A9097);
/// Captions, uppercase labels, placeholders (13 px and up).
pub const GRAY: Rgba = hex(0x747B82);
/// Disabled glyphs, inactive dots.
pub const DIM: Rgba = hex(0x4A4F56);
/// The light: focused, running, selected, yours. Use sparingly.
pub const AMBER: Rgba = hex(0xC59B60);
/// Hover on amber things.
pub const AMBER_HI: Rgba = hex(0xDDB97F);
/// The hot centre of the slit and other glows.
pub const CORE: Rgba = hex(0xF6E9D2);
pub const SUCCESS: Rgba = hex(0x7FAF8C);
pub const DANGER: Rgba = hex(0xD0796B);
pub const INFO: Rgba = hex(0x7E9FBF);
/// Light-mode background (docs, print).
pub const PAPER: Rgba = hex(0xEEECE6);

/// Things that must be told apart (players, series), in order. Never amber.
pub const CATEGORICAL: [Rgba; 6] = [
    hex(0x7E9FBF), // steel
    hex(0xB9789A), // mauve
    hex(0x7FAF8C), // sage
    hex(0x9A8CC4), // lavender
    hex(0x6FB0A8), // teal
    hex(0xC9826B), // clay
];

/// The categorical colour for a stable key (a player id, a series name).
pub fn categorical(key: u64) -> Rgba {
    // A tiny mix so neighbouring ids don't get neighbouring colours.
    let h = key.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 59;
    CATEGORICAL[(h as usize) % CATEGORICAL.len()]
}

// ── Tints (BRAND.md §3) ───────────────────────────────────────────────────────

pub const SELECTED_BG: Rgba = alpha(AMBER, 0.10);
pub const FOCUS_BORDER: Rgba = alpha(AMBER, 0.35);
pub const TOPBAR_RULE: Rgba = alpha(AMBER, 0.18);
pub const HOVER_BG: Rgba = alpha(IVORY, 0.04);
pub const WINDOW_BORDER: Rgba = alpha(IVORY, 0.08);
pub const INPUT_BORDER: Rgba = alpha(IVORY, 0.12);
pub const SCRIM: Rgba = alpha(INK, 0.72);

// ── Type, space, shape (BRAND.md §4, §5) ──────────────────────────────────────

pub const TEXT_CAPTION: f32 = 11.0;
pub const TEXT_SMALL: f32 = 12.0;
pub const TEXT_BODY: f32 = 13.0;
pub const TEXT_READING: f32 = 15.0;
pub const TEXT_TITLE: f32 = 17.0;
pub const TEXT_HEADING: f32 = 22.0;
pub const TEXT_DISPLAY: f32 = 34.0;

/// Tracking of uppercase labels, in em.
pub const LABEL_TRACKING: f32 = 0.18;
/// Tracking of the wordmark, in em.
pub const WORDMARK_TRACKING: f32 = 0.32;
/// Tracking of the tagline, in em.
pub const TAGLINE_TRACKING: f32 = 0.22;
pub const TAGLINE: &str = "A SPACE FOR THINGS TO INTERACT";

pub const SPACE_1: f32 = 4.0;
pub const SPACE_2: f32 = 8.0;
pub const SPACE_3: f32 = 12.0;
pub const SPACE_4: f32 = 16.0;
pub const SPACE_6: f32 = 24.0;
pub const SPACE_8: f32 = 32.0;
pub const SPACE_12: f32 = 48.0;

pub const RADIUS_CHIP: f32 = 4.0;
pub const RADIUS_CONTROL: f32 = 6.0;
pub const RADIUS_WINDOW: f32 = 10.0;
pub const RADIUS_TILE: f32 = 16.0;

// ── Tracked text ──────────────────────────────────────────────────────────────

/// Pen x of each char of `s` set with `tracking_em` extra space after every
/// char but the last, starting at 0. The engine's text has no letter-spacing,
/// so tracked text (labels, the wordmark) is drawn char by char at these
/// offsets. Returns the pieces and the total width.
pub fn tracked(s: &str, size: f32, tracking_em: f32, style: FontStyle) -> (Vec<(char, f32)>, f32) {
    let mut x = 0.0;
    let n = s.chars().count();
    let mut out = Vec::with_capacity(n);
    for (i, ch) in s.chars().enumerate() {
        out.push((ch, x));
        x += char_advance(ch, size, style);
        if i + 1 < n {
            x += tracking_em * size;
        }
    }
    (out, x)
}

/// An uppercase label (BRAND.md §4) as plain text for UI elements: the
/// letters joined by thin spaces (U+2009, 0.187 em in Space Grotesk), which is
/// the labels' +0.18 em tracking without letter-spacing support in the engine.
/// Draw it at `TEXT_CAPTION` in `GRAY`.
pub fn label(s: &str) -> String {
    let mut out = String::new();
    for (i, ch) in s.to_uppercase().chars().enumerate() {
        if i > 0 {
            out.push('\u{2009}');
        }
        out.push(ch);
    }
    out
}

/// Cap height of the UI face at `size`: Space Grotesk's is 0.70 em.
pub fn cap_height(size: f32) -> f32 {
    0.70 * size
}

/// Layout of the wordmark (BRAND.md §2) for letters of `size` px: where the
/// slit and the letters go, relative to the top-left of the lockup.
pub struct Wordmark {
    /// Slit rect: centre x, top y, height. Its texture is drawn
    /// `SLIT_ASPECT × height` wide, centred on `slit_cx`.
    pub slit_cx: f32,
    pub slit_top: f32,
    pub slit_h: f32,
    /// Top y of the letters' line box (where `draw_text` puts them).
    pub text_top: f32,
    /// The letters `NTERSTICE`, with their x offsets.
    pub letters: Vec<(char, f32)>,
    pub width: f32,
    pub height: f32,
}

pub fn wordmark(size: f32) -> Wordmark {
    let cap = cap_height(size);
    let slit_h = 2.1 * cap;
    let (letters, w) = tracked("NTERSTICE", size, WORDMARK_TRACKING, FontStyle::REGULAR);
    let slit_cx = SLIT_ASPECT * slit_h * 0.5;
    let x0 = slit_cx + 0.62 * cap;
    let ascent = crate::text::ascent(size, FontStyle::REGULAR);
    // Letters' cap centre at mid-height; the slit is centred on it.
    let height = slit_h;
    let cap_mid = height / 2.0;
    let text_top = cap_mid + cap / 2.0 - ascent;
    Wordmark {
        slit_cx,
        slit_top: 0.0,
        slit_h,
        text_top,
        letters: letters.into_iter().map(|(c, x)| (c, x0 + x)).collect(),
        width: x0 + w,
        height,
    }
}

// ── Brand textures ────────────────────────────────────────────────────────────

/// Width / height of the slit texture (it carries its glow).
pub const SLIT_ASPECT: f32 = 0.25;

/// The slit (BRAND.md §2) as an RGBA8 texture `w × h`: a lens of light, pointed
/// at both ends, with a `CORE` centre fading to `AMBER` at its tips and edges
/// and an amber glow around it. `glow` scales the glow's strength (1.0 nominal;
/// the breathing animation varies the tint alpha instead).
pub fn slit_rgba(w: u32, h: u32, glow: f32) -> Vec<u8> {
    let (wf, hf) = (w as f32, h as f32);
    let cx = (wf - 1.0) / 2.0;
    // Core half-width at the middle: a hairline (1.8 % of H), never under
    // 0.8 px so small slits stay visible.
    let half = (0.009 * hf).max(0.8);
    let sigma = (0.022 * hf).max(1.6);
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        // Lens profile: 4t(1-t), widest at the middle, 0 at the ends.
        let t = (y as f32 + 0.5) / hf;
        let lens = (4.0 * t * (1.0 - t)).max(0.0);
        let hw = half * lens; // core half-width on this row
        let heat = lens.powf(1.6); // how close to the hot middle
        for x in 0..w {
            let d = (x as f32 - cx).abs();
            // Core: AA edge at hw.
            let core_a = ((hw - d) + 0.5).clamp(0.0, 1.0) * lens.powf(0.35);
            // Glow: a Gaussian across the lens, strongest at the middle.
            let gd = (d - hw).max(0.0);
            let glow_a = glow * 0.62 * (-(gd * gd) / (2.0 * sigma * sigma)).exp() * lens.powf(1.2);
            // Bloom: a very wide faint ellipse.
            let (bx, by) = (d / (0.5 * wf), (t - 0.5) / 0.48);
            let bloom_a = glow * 0.10 * (1.0 - (bx * bx + by * by)).max(0.0).powf(2.0);
            let ga = (glow_a + bloom_a).min(1.0);
            let core_c = mix(AMBER, CORE, heat);
            let a = core_a + ga * (1.0 - core_a);
            let px = if a <= 0.0 {
                (0.0, 0.0, 0.0)
            } else {
                let k = ga * (1.0 - core_a);
                (
                    (core_c.0 * core_a + AMBER.0 * k) / a,
                    (core_c.1 * core_a + AMBER.1 * k) / a,
                    (core_c.2 * core_a + AMBER.2 * k) / a,
                )
            };
            let i = ((y * w + x) * 4) as usize;
            out[i] = (px.0 * 255.0) as u8;
            out[i + 1] = (px.1 * 255.0) as u8;
            out[i + 2] = (px.2 * 255.0) as u8;
            out[i + 3] = (a.clamp(0.0, 1.0) * 255.0) as u8;
        }
    }
    out
}

/// An RGBA8 image: `width × height`, tightly packed, straight alpha.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn new(width: u32, height: u32) -> Self {
        Image { width, height, rgba: vec![0; (width * height * 4) as usize] }
    }

    /// Composite `colour` at coverage `cov` over pixel (x, y).
    fn blend(&mut self, x: i32, y: i32, colour: Rgba, cov: f32) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let i = ((y as u32 * self.width + x as u32) * 4) as usize;
        let k = (cov * colour.3).clamp(0.0, 1.0);
        let da = self.rgba[i + 3] as f32 / 255.0;
        let oa = k + da * (1.0 - k);
        for (c, src) in [colour.0, colour.1, colour.2].into_iter().enumerate() {
            let dst = self.rgba[i + c] as f32 / 255.0;
            let v = if oa > 0.0 { (src * k + dst * da * (1.0 - k)) / oa } else { 0.0 };
            self.rgba[i + c] = (v * 255.0).round() as u8;
        }
        self.rgba[i + 3] = (oa * 255.0).round() as u8;
    }

    /// Draw another image over this one at (x, y).
    pub fn draw(&mut self, src: &Image, x: i32, y: i32) {
        for sy in 0..src.height {
            for sx in 0..src.width {
                let i = ((sy * src.width + sx) * 4) as usize;
                let p = &src.rgba[i..i + 4];
                if p[3] > 0 {
                    let c = (p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0, 1.0);
                    self.blend(x + sx as i32, y + sy as i32, c, p[3] as f32 / 255.0);
                }
            }
        }
    }

    /// Rasterize `s` with `tracking_em` letter-spacing; (x, y) is the top of
    /// the line box, as for `draw_text`.
    pub fn text(&mut self, s: &str, x: f32, y: f32, size: f32, tracking_em: f32, style: FontStyle, colour: Rgba) {
        use ab_glyph::{Font, PxScale, point};
        let (pieces, _) = tracked(s, size, tracking_em, style);
        let baseline = y + crate::text::ascent(size, style);
        for (ch, dx) in pieces {
            let font = crate::text::face_for(ch, style);
            let glyph = font.glyph_id(ch).with_scale_and_position(PxScale::from(size), point(x + dx, baseline));
            if let Some(og) = font.outline_glyph(glyph) {
                let b = og.px_bounds();
                og.draw(|gx, gy, c| self.blend(b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32, colour, c));
            }
        }
    }
}

/// Size of a tracked line of text: (width, line height), rounded up.
pub fn text_size(s: &str, size: f32, tracking_em: f32, style: FontStyle) -> (u32, u32) {
    let (_, w) = tracked(s, size, tracking_em, style);
    (w.ceil() as u32 + 2, crate::text::text_line_height(size, style).ceil() as u32)
}

/// A tracked line of text as an image (labels and lines that never change).
pub fn text_image(s: &str, size: f32, tracking_em: f32, style: FontStyle, colour: Rgba) -> Image {
    let (w, h) = text_size(s, size, tracking_em, style);
    let mut img = Image::new(w, h);
    img.text(s, 0.0, 0.0, size, tracking_em, style, colour);
    img
}

/// The wordmark (BRAND.md §2) for letters of `size` px as an image: the slit
/// with its glow, then `NTERSTICE` in `letters`. The slit's glow spills past
/// the letters' box, so the image has a margin of `pad` px on every side;
/// draw it at `(x - pad, y - pad)` to put the lockup's box at (x, y).
pub fn wordmark_image(size: f32, letters: Rgba) -> (Image, f32) {
    let wm = wordmark(size);
    let slit_w = (SLIT_ASPECT * wm.slit_h * 2.0).ceil();
    let pad = (slit_w / 2.0).ceil();
    let mut img = Image::new((wm.width + 2.0 * pad).ceil() as u32 + 2, (wm.height + 2.0 * pad).ceil() as u32);
    // The slit: rendered tall, with a wide glow box, centred on slit_cx.
    let sh = wm.slit_h.ceil() as u32;
    let sw = slit_w as u32 | 1;
    let slit = Image { width: sw, height: sh, rgba: slit_rgba_wide(sw, sh, 1.0, wm.slit_h * SLIT_ASPECT) };
    img.draw(&slit, (pad + wm.slit_cx - sw as f32 / 2.0).round() as i32, pad as i32);
    let mut pieces = String::new();
    pieces.extend(wm.letters.iter().map(|(c, _)| *c));
    let x0 = wm.letters.first().map_or(0.0, |l| l.1);
    img.text(&pieces, pad + x0, pad + wm.text_top, size, WORDMARK_TRACKING, FontStyle::REGULAR, letters);
    (img, pad)
}

/// [`slit_rgba`] in a texture wider than the slit's own aspect: the lens and
/// glow keep the proportions of a `lens_w`-wide slit, centred.
fn slit_rgba_wide(w: u32, h: u32, glow: f32, lens_w: f32) -> Vec<u8> {
    let inner_w = (lens_w.round() as u32).clamp(1, w) | 1;
    let inner = slit_rgba(inner_w, h, glow);
    let mut img = Image::new(w, h);
    img.draw(&Image { width: inner_w, height: h, rgba: inner }, ((w - inner_w) / 2) as i32, 0);
    img.rgba
}

/// A soft round glow (`colour` at the centre fading to transparent at the
/// edge), `n × n`. Drawn stretched for halos: your agar cell, a lit dot.
pub fn glow_rgba(n: u32, colour: Rgba) -> Vec<u8> {
    let c = (n as f32 - 1.0) / 2.0;
    let mut out = vec![0u8; (n * n * 4) as usize];
    for y in 0..n {
        for x in 0..n {
            let (dx, dy) = ((x as f32 - c) / c, (y as f32 - c) / c);
            let r = (dx * dx + dy * dy).sqrt();
            let a = (1.0 - r).max(0.0).powf(2.2);
            let i = ((y * n + x) * 4) as usize;
            out[i] = (colour.0 * 255.0) as u8;
            out[i + 1] = (colour.1 * 255.0) as u8;
            out[i + 2] = (colour.2 * 255.0) as u8;
            out[i + 3] = (a * colour.3 * 255.0) as u8;
        }
    }
    out
}

/// A tiny anti-aliased vector canvas for monochrome brand glyphs (launcher
/// icons). Coordinates are on a 0..1 square; strokes have round caps.
pub struct Canvas {
    n: u32,
    cov: Vec<f32>,
}

impl Canvas {
    pub fn new(n: u32) -> Self {
        Canvas { n, cov: vec![0.0; (n * n) as usize] }
    }

    fn paint(&mut self, sdf: impl Fn(f32, f32) -> f32) {
        let n = self.n as f32;
        let px = 1.0 / n;
        for y in 0..self.n {
            for x in 0..self.n {
                let (u, v) = ((x as f32 + 0.5) / n, (y as f32 + 0.5) / n);
                let a = (0.5 - sdf(u, v) / px).clamp(0.0, 1.0);
                let c = &mut self.cov[(y * self.n + x) as usize];
                *c = c.max(a);
            }
        }
    }

    /// A stroked segment.
    pub fn line(&mut self, (x0, y0): (f32, f32), (x1, y1): (f32, f32), width: f32) {
        self.paint(|u, v| {
            let (dx, dy) = (x1 - x0, y1 - y0);
            let t = (((u - x0) * dx + (v - y0) * dy) / (dx * dx + dy * dy).max(1e-9)).clamp(0.0, 1.0);
            let (px, py) = (x0 + t * dx - u, y0 + t * dy - v);
            (px * px + py * py).sqrt() - width / 2.0
        });
    }

    /// A stroked circle.
    pub fn ring(&mut self, (cx, cy): (f32, f32), r: f32, width: f32) {
        self.paint(|u, v| (((u - cx).powi(2) + (v - cy).powi(2)).sqrt() - r).abs() - width / 2.0);
    }

    /// A filled circle.
    pub fn disc(&mut self, (cx, cy): (f32, f32), r: f32) {
        self.paint(|u, v| ((u - cx).powi(2) + (v - cy).powi(2)).sqrt() - r);
    }

    /// A stroked rounded rectangle.
    pub fn rrect(&mut self, (x, y): (f32, f32), (w, h): (f32, f32), radius: f32, width: f32) {
        self.paint(|u, v| {
            let (cx, cy) = (x + w / 2.0, y + h / 2.0);
            let qx = (u - cx).abs() - (w / 2.0 - radius);
            let qy = (v - cy).abs() - (h / 2.0 - radius);
            let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
            ((outside + qx.max(qy).min(0.0)) - radius).abs() - width / 2.0
        });
    }

    /// A filled polygon (even-odd), edges anti-aliased.
    pub fn polygon(&mut self, pts: &[(f32, f32)]) {
        let pts = pts.to_vec();
        self.paint(move |u, v| {
            let mut d = f32::MAX;
            let mut inside = false;
            for i in 0..pts.len() {
                let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let t = (((u - a.0) * dx + (v - a.1) * dy) / (dx * dx + dy * dy).max(1e-9)).clamp(0.0, 1.0);
                let (px, py) = (a.0 + t * dx - u, a.1 + t * dy - v);
                d = d.min((px * px + py * py).sqrt());
                if (a.1 > v) != (b.1 > v) && u < a.0 + (v - a.1) / (b.1 - a.1) * dx {
                    inside = !inside;
                }
            }
            if inside { -d } else { d }
        });
    }

    /// A stroked polyline through `pts`.
    pub fn path(&mut self, pts: &[(f32, f32)], width: f32) {
        for w in pts.windows(2) {
            self.line(w[0], w[1], width);
        }
    }

    /// The glyph as RGBA8 in `colour` on `background` (straight alpha).
    pub fn to_rgba(&self, colour: Rgba, background: Rgba) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.cov.len() * 4);
        for &a in &self.cov {
            let k = a * colour.3;
            let oa = k + background.3 * (1.0 - k);
            let ch = |c: f32, b: f32| {
                if oa <= 0.0 { 0.0 } else { (c * k + b * background.3 * (1.0 - k)) / oa }
            };
            out.push((ch(colour.0, background.0) * 255.0) as u8);
            out.push((ch(colour.1, background.1) * 255.0) as u8);
            out.push((ch(colour.2, background.2) * 255.0) as u8);
            out.push((oa * 255.0) as u8);
        }
        out
    }
}

/// Several canvases of the same size composited bottom to top, each in its
/// colour, over a transparent background.
pub fn compose(layers: &[(&Canvas, Rgba)]) -> Vec<u8> {
    let len = layers.first().map_or(0, |(c, _)| c.cov.len());
    let mut out = Vec::with_capacity(len * 4);
    for i in 0..len {
        let (mut r, mut g, mut b, mut a) = (0.0f32, 0.0f32, 0.0f32, 0.0f32); // premultiplied
        for (canvas, c) in layers {
            let k = canvas.cov[i] * c.3;
            r = c.0 * k + r * (1.0 - k);
            g = c.1 * k + g * (1.0 - k);
            b = c.2 * k + b * (1.0 - k);
            a = k + a * (1.0 - k);
        }
        let un = |x: f32| if a > 0.0 { (x / a * 255.0) as u8 } else { 0 };
        out.extend_from_slice(&[un(r), un(g), un(b), (a * 255.0) as u8]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slit_is_bright_in_the_middle_and_empty_at_the_ends() {
        let (w, h) = (32, 128);
        let px = slit_rgba(w, h, 1.0);
        let at = |x: u32, y: u32| px[((y * w + x) * 4 + 3) as usize];
        assert!(at(w / 2, h / 2) > 240);
        assert_eq!(at(0, 0), 0);
        assert!(at(w / 2, 0) < 40);
    }

    #[test]
    fn tracking_adds_space_between_letters_only() {
        let (pieces, w) = tracked("AB", 10.0, 0.5, FontStyle::REGULAR);
        let a = char_advance('A', 10.0, FontStyle::REGULAR);
        let b = char_advance('B', 10.0, FontStyle::REGULAR);
        assert_eq!(pieces[1].1, a + 5.0);
        assert!((w - (a + b + 5.0)).abs() < 1e-4);
    }
}
