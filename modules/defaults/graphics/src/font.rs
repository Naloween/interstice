//! Real-font text: a glyph atlas rasterized from the *same* embedded face and
//! the *same* `PxScale::from(size)` convention the UI engine
//! (`interstice-ui::text`) measures with. Because both sides compute advances
//! from identical font bytes the same way, the glyph positions drawn here line
//! up exactly with the wrapping/alignment the engine laid out.

use ab_glyph::{Font, FontArc, PxScale, ScaleFont};
use std::collections::HashMap;
use std::sync::OnceLock;

/// The single embedded UI font (DejaVu Sans) — same bytes the engine measures
/// with (kept in sync via the matching copy in `crates/interstice-ui/assets`).
pub const FONT_TTF: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");

/// Pixels/em the atlas is rasterized at. Drawing scales each cached glyph by
/// `size / ATLAS_BASE_PX`, so this is the quality ceiling for large text; 48px
/// keeps body text crisp without an oversized texture.
pub const ATLAS_BASE_PX: f32 = 48.0;

fn font() -> &'static FontArc {
    static FONT: OnceLock<FontArc> = OnceLock::new();
    FONT.get_or_init(|| FontArc::try_from_slice(FONT_TTF).expect("embedded DejaVuSans is valid"))
}

/// Horizontal advance of a single character at `size` px/em. MUST match
/// `interstice_ui::char_advance` exactly so drawn pen positions track layout.
pub fn char_advance(ch: char, size: f32) -> f32 {
    if size <= 0.0 {
        return 0.0;
    }
    let sf = font().as_scaled(PxScale::from(size));
    sf.h_advance(sf.glyph_id(ch))
}

/// Distance from the line's top to the text baseline at `size`.
pub fn ascent(size: f32) -> f32 {
    if size <= 0.0 {
        return 0.0;
    }
    font().as_scaled(PxScale::from(size)).ascent()
}

/// Line advance — matches `interstice_ui::text_line_height`.
pub fn text_line_height(size: f32) -> f32 {
    if size <= 0.0 {
        return 0.0;
    }
    let sf = font().as_scaled(PxScale::from(size));
    (sf.height() + sf.line_gap()).ceil()
}

/// A glyph's placement in the atlas plus its bitmap box, all at base scale.
#[derive(Clone, Copy)]
pub struct GlyphInfo {
    /// UV rect of the glyph's bitmap within the atlas texture (0..1).
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
    /// Bitmap box, in base-px, relative to the pen origin on the baseline.
    /// `left`/`top` are the bearings (top is negative for ink above baseline).
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

pub struct Atlas {
    pub width: u32,
    pub height: u32,
    /// Tightly-packed RGBA8: white pixels with alpha = coverage, so the textured
    /// pipeline (`tex * tint`, premultiply-free straight alpha) tints AA text.
    pub rgba: Vec<u8>,
    glyphs: HashMap<char, GlyphInfo>,
}

impl Atlas {
    pub fn glyph(&self, ch: char) -> Option<&GlyphInfo> {
        self.glyphs.get(&ch)
    }
}

/// The set of characters baked into the atlas: ASCII printable, Latin-1
/// supplement, and the curated punctuation real pages lean on (smart quotes,
/// dashes, ellipsis, bullet, euro/trademark, arrows). Anything outside this set
/// still advances by its true width (layout stays correct) but draws blank.
fn charset() -> Vec<char> {
    let mut v: Vec<char> = Vec::new();
    for c in 0x20u32..0x7f {
        if let Some(ch) = char::from_u32(c) {
            v.push(ch);
        }
    }
    for c in 0xa0u32..0x100 {
        if let Some(ch) = char::from_u32(c) {
            v.push(ch);
        }
    }
    v.extend([
        '\u{2013}', '\u{2014}', // – —
        '\u{2018}', '\u{2019}', // ‘ ’
        '\u{201c}', '\u{201d}', // “ ”
        '\u{2022}', '\u{2026}', // • …
        '\u{2039}', '\u{203a}', // ‹ ›
        '\u{20ac}', '\u{2122}', // € ™
        '\u{2190}', '\u{2192}', // ← →
        '\u{00a0}', // nbsp (drawn blank but keeps width)
    ]);
    v
}

/// Build the glyph atlas once: rasterize every charset glyph at `ATLAS_BASE_PX`
/// into a padded grid, recording each glyph's UV rect + bearings/size.
pub fn atlas() -> &'static Atlas {
    static ATLAS: OnceLock<Atlas> = OnceLock::new();
    ATLAS.get_or_init(build_atlas)
}

fn build_atlas() -> Atlas {
    let f = font();
    let chars = charset();

    // First pass: rasterize each glyph to its own coverage buffer, recording
    // its size + bearings so we can both pack it and place it when drawing.
    struct Raster {
        ch: char,
        w: u32,
        h: u32,
        left: f32,
        top: f32,
        cov: Vec<u8>,
    }
    let mut rasters: Vec<Raster> = Vec::with_capacity(chars.len());
    let mut max_w = 1u32;
    let mut max_h = 1u32;
    for ch in chars {
        let glyph = f.glyph_id(ch).with_scale(ATLAS_BASE_PX);
        if let Some(outlined) = f.outline_glyph(glyph) {
            let b = outlined.px_bounds();
            let w = b.width().ceil().max(0.0) as u32;
            let h = b.height().ceil().max(0.0) as u32;
            if w == 0 || h == 0 {
                rasters.push(Raster { ch, w: 0, h: 0, left: 0.0, top: 0.0, cov: Vec::new() });
                continue;
            }
            let mut cov = vec![0u8; (w * h) as usize];
            outlined.draw(|x, y, c| {
                let idx = (y * w + x) as usize;
                if idx < cov.len() {
                    cov[idx] = (c * 255.0).round().clamp(0.0, 255.0) as u8;
                }
            });
            max_w = max_w.max(w);
            max_h = max_h.max(h);
            rasters.push(Raster { ch, w, h, left: b.min.x, top: b.min.y, cov });
        } else {
            // No outline (e.g. space): width-only, no bitmap.
            rasters.push(Raster { ch, w: 0, h: 0, left: 0.0, top: 0.0, cov: Vec::new() });
        }
    }

    // Pack into a fixed-column grid with 2px padding between cells (avoids
    // bilinear bleed between neighbours). Round the atlas width up to a multiple
    // of 64 so the row stride (width*4) is 256-aligned for the texture upload.
    const PAD: u32 = 2;
    const COLS: u32 = 16;
    let cell_w = max_w + PAD * 2;
    let cell_h = max_h + PAD * 2;
    let rows = (rasters.len() as u32).div_ceil(COLS);
    let aw = (COLS * cell_w).div_ceil(64) * 64;
    let ah = rows * cell_h;
    let mut rgba = vec![0u8; (aw * ah * 4) as usize];
    let mut glyphs: HashMap<char, GlyphInfo> = HashMap::new();

    for (i, r) in rasters.iter().enumerate() {
        let col = (i as u32) % COLS;
        let row = (i as u32) / COLS;
        let ox = col * cell_w + PAD;
        let oy = row * cell_h + PAD;
        for gy in 0..r.h {
            for gx in 0..r.w {
                let cov = r.cov[(gy * r.w + gx) as usize];
                let px = (((oy + gy) * aw + (ox + gx)) * 4) as usize;
                rgba[px] = 255;
                rgba[px + 1] = 255;
                rgba[px + 2] = 255;
                rgba[px + 3] = cov;
            }
        }
        glyphs.insert(
            r.ch,
            GlyphInfo {
                u0: ox as f32 / aw as f32,
                v0: oy as f32 / ah as f32,
                u1: (ox + r.w) as f32 / aw as f32,
                v1: (oy + r.h) as f32 / ah as f32,
                left: r.left,
                top: r.top,
                width: r.w as f32,
                height: r.h as f32,
            },
        );
    }

    Atlas { width: aw, height: ah, rgba, glyphs }
}
