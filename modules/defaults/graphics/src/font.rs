//! Real-font text: a glyph atlas rasterized from the *same* embedded face and
//! the *same* `PxScale::from(size)` convention the UI engine
//! (`interstice-ui::text`) measures with. Because both sides compute advances
//! from identical font bytes the same way, the glyph positions drawn here line
//! up exactly with the wrapping/alignment the engine laid out.

use ab_glyph::{Font, FontArc, PxScale, ScaleFont};
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;

/// The four embedded UI faces (brand/BRAND.md §4): Space Grotesk Regular/Medium
/// and IBM Plex Mono Regular/Medium — the same bytes the engine measures with
/// (kept in sync via the matching copies in `crates/interstice-ui/assets`).
pub const FONT_TTF: &[u8] = include_bytes!("../assets/SpaceGrotesk-Regular.ttf");
pub const FONT_TTF_BOLD: &[u8] = include_bytes!("../assets/SpaceGrotesk-Medium.ttf");
pub const FONT_TTF_MONO: &[u8] = include_bytes!("../assets/IBMPlexMono-Regular.ttf");
pub const FONT_TTF_MONO_BOLD: &[u8] = include_bytes!("../assets/IBMPlexMono-Medium.ttf");
/// DejaVu Sans, for what the brand faces lack (Greek, Cyrillic, symbols).
pub const FONT_TTF_FALLBACK: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");
pub const FONT_TTF_FALLBACK_BOLD: &[u8] = include_bytes!("../assets/DejaVuSans-Bold.ttf");
/// CJK fallback (Droid Sans Fallback) — the *same* bytes the engine measures with
/// (`interstice-ui/assets`), so glyphs it supplies advance identically on both
/// sides. Single weight, used for every style.
pub const FONT_TTF_CJK: &[u8] = include_bytes!("../assets/DroidSansFallbackFull.ttf");

/// Pixels/em the atlas is rasterized at. Drawing scales each cached glyph by
/// `size / ATLAS_BASE_PX`, so this is the quality ceiling for large text; 48px
/// keeps body text crisp without an oversized texture.
pub const ATLAS_BASE_PX: f32 = 48.0;

/// Number of distinct weight/family combinations (and thus faces/atlases).
pub const STYLE_COUNT: usize = 4;

/// 0=regular 1=medium 2=mono 3=mono-medium. Matches the order the renderer
/// caches atlas views and the encoding the engine's draw forwarder packs into
/// the `font` field of a text command (see [`parse_style`]).
pub fn style_index(bold: bool, mono: bool) -> usize {
    (bold as usize) | ((mono as usize) << 1)
}

/// Decode the `font` selector the engine sends ("bold"/"mono"/"mono-bold"/
/// none) into a `(bold, mono)` pair. The old "italic" selectors draw upright:
/// the brand has no italics.
pub fn parse_style(font: Option<&str>) -> (bool, bool) {
    match font {
        Some("bold") | Some("bold-italic") => (true, false),
        Some("mono") => (false, true),
        Some("mono-bold") => (true, true),
        _ => (false, false),
    }
}

fn parse(bytes: &'static [u8]) -> FontArc {
    FontArc::try_from_slice(bytes).expect("embedded face is valid")
}

fn face(bold: bool, mono: bool) -> &'static FontArc {
    static FACES: [OnceLock<FontArc>; STYLE_COUNT] = [
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
        OnceLock::new(),
    ];
    let idx = style_index(bold, mono);
    FACES[idx].get_or_init(|| parse([FONT_TTF, FONT_TTF_BOLD, FONT_TTF_MONO, FONT_TTF_MONO_BOLD][idx]))
}

/// DejaVu, for characters the brand faces don't cover.
fn fallback_face(bold: bool) -> &'static FontArc {
    static REG: OnceLock<FontArc> = OnceLock::new();
    static BOLD: OnceLock<FontArc> = OnceLock::new();
    match bold {
        false => REG.get_or_init(|| parse(FONT_TTF_FALLBACK)),
        true => BOLD.get_or_init(|| parse(FONT_TTF_FALLBACK_BOLD)),
    }
}

/// The shared CJK fallback face (one weight for all styles).
fn cjk_face() -> &'static FontArc {
    static CJK: OnceLock<FontArc> = OnceLock::new();
    CJK.get_or_init(|| parse(FONT_TTF_CJK))
}

/// Pick the face that has a glyph for `ch`: the brand face for the style, else
/// DejaVu, else the CJK fallback. Mirrors `interstice_ui::face_for` exactly.
fn face_for(ch: char, bold: bool, mono: bool) -> &'static FontArc {
    let primary = face(bold, mono);
    if primary.glyph_id(ch).0 != 0 {
        return primary;
    }
    let fallback = fallback_face(bold);
    if fallback.glyph_id(ch).0 != 0 {
        return fallback;
    }
    if cjk_face().glyph_id(ch).0 != 0 { cjk_face() } else { primary }
}

/// Horizontal advance of a single character at `size` px/em in the given style.
/// MUST match `interstice_ui::char_advance` exactly so drawn pen positions track
/// layout.
pub fn char_advance(ch: char, size: f32, bold: bool, mono: bool) -> f32 {
    if size <= 0.0 {
        return 0.0;
    }
    let sf = face_for(ch, bold, mono).as_scaled(PxScale::from(size));
    sf.h_advance(sf.glyph_id(ch))
}

/// Distance from the line's top to the text baseline at `size`.
pub fn ascent(size: f32, bold: bool, mono: bool) -> f32 {
    if size <= 0.0 {
        return 0.0;
    }
    face(bold, mono).as_scaled(PxScale::from(size)).ascent()
}

/// Line advance — matches `interstice_ui::text_line_height`.
pub fn text_line_height(size: f32, bold: bool, mono: bool) -> f32 {
    if size <= 0.0 {
        return 0.0;
    }
    let sf = face(bold, mono).as_scaled(PxScale::from(size));
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
/// supplement, Greek and Cyrillic (both fully covered by DejaVu Sans — so
/// language names like `Ελληνικά`/`Русский` render), and the curated punctuation
/// real pages lean on (smart quotes, dashes, ellipsis, bullet, euro/trademark,
/// arrows). Anything outside this set (e.g. CJK, which would need a separate,
/// much larger font) still advances by its true width (layout stays correct) but
/// draws blank.
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
    // Greek, Cyrillic, and every other non-Latin script (CJK, Arabic, Devanagari,
    // …) are baked LAZILY via `note_str`/`ensure_built` — only the characters a
    // page actually uses. Pre-baking the full Greek+Cyrillic blocks here (400
    // glyphs × 4 styles) dominated first-page load for scripts most pages never
    // show, so they now go through the same on-demand path as CJK.
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

/// Cap on the number of *extra* (beyond the base charset) glyphs we bake — one
/// distinct CJK/other character each. Generous enough for any real page while
/// keeping the packed texture well under the GPU's max dimension.
const MAX_EXTRA_GLYPHS: usize = 4096;

/// The four style atlases plus the growable set of extra characters (CJK etc.)
/// they cover. The base charset is always baked; extra characters are added
/// lazily the first frame they appear (`note_str` → `ensure_built`), which bumps
/// `generation` so the renderer re-uploads the textures. wasm modules are
/// single-threaded, so this `thread_local` is contention-free and persists across
/// frames for the life of the module instance.
struct AtlasSet {
    /// `slot * STYLE_COUNT + style_index`: slot 0 is the base atlas
    /// (`ATLAS_BASE_PX`), slot k ≥ 1 is rasterized at `sizes[k - 1]` px.
    atlases: Vec<Atlas>,
    sizes: Vec<u32>,
    pending_sizes: BTreeSet<u32>,
    covered: BTreeSet<char>,
    pending: BTreeSet<char>,
    generation: u32,
    built: bool,
}

thread_local! {
    static ATLAS_SET: RefCell<AtlasSet> = RefCell::new(AtlasSet {
        atlases: Vec::new(),
        sizes: Vec::new(),
        pending_sizes: BTreeSet::new(),
        covered: BTreeSet::new(),
        pending: BTreeSet::new(),
        generation: 0,
        built: false,
    });
}

/// Record characters seen in this frame's text so the next `ensure_built` bakes
/// any not-yet-covered ones. Only characters the CJK fallback supplies (i.e. not
/// in the base charset, but a real glyph) are queued — everything else is either
/// already baked or unrenderable.
pub fn note_str(s: &str) {
    ATLAS_SET.with(|cell| {
        let mut set = cell.borrow_mut();
        if set.covered.len() >= MAX_EXTRA_GLYPHS {
            return;
        }
        for ch in s.chars() {
            if set.covered.contains(&ch) || set.pending.contains(&ch) || in_base_charset(ch) {
                continue;
            }
            // Bake any char some face can render but the (now Latin-only) base
            // charset omits — Greek/Cyrillic via DejaVu, CJK via the fallback.
            if face(false, false).glyph_id(ch).0 != 0
                || fallback_face(false).glyph_id(ch).0 != 0
                || cjk_face().glyph_id(ch).0 != 0 {
                set.pending.insert(ch);
                if set.covered.len() + set.pending.len() >= MAX_EXTRA_GLYPHS {
                    break;
                }
            }
        }
    });
}

/// (Re)build the four atlases if the base bake hasn't happened yet or new extra
/// characters were queued. Returns the (possibly bumped) generation.
pub fn ensure_built() -> u32 {
    ATLAS_SET.with(|cell| {
        let mut set = cell.borrow_mut();
        if set.built && set.pending.is_empty() && set.pending_sizes.is_empty() {
            return set.generation;
        }
        let pending: Vec<char> = set.pending.iter().copied().collect();
        for ch in pending {
            set.covered.insert(ch);
        }
        set.pending.clear();
        let new_sizes: Vec<u32> = set.pending_sizes.iter().copied().collect();
        set.sizes.extend(new_sizes);
        set.pending_sizes.clear();
        let extra: Vec<char> = set.covered.iter().copied().collect();
        let mut atlases = Vec::with_capacity(STYLE_COUNT * (1 + set.sizes.len()));
        let slot_px: Vec<f32> =
            std::iter::once(ATLAS_BASE_PX).chain(set.sizes.iter().map(|&s| s as f32)).collect();
        for px in slot_px {
            for idx in 0..STYLE_COUNT {
                atlases.push(build_atlas(idx & 1 != 0, idx & 2 != 0, px, &extra));
            }
        }
        set.atlases = atlases;
        set.built = true;
        set.generation += 1;
        set.generation
    })
}

/// Smallest and largest sizes that get an atlas rasterized at exactly their
/// size, and how many such sizes are kept. UI text uses a handful of integer
/// sizes (BRAND.md §4); a glyph drawn at its own size, on whole pixels, is
/// crisp, whereas one scaled down from the base atlas loses thin stems.
/// Other sizes (fractional, huge, or past the cap) scale the base atlas.
const EXACT_MIN_PX: u32 = 8;
const EXACT_MAX_PX: u32 = 40;
const MAX_EXACT_SIZES: usize = 16;

/// Queue an exact-size atlas for `size` if it qualifies (see `EXACT_MAX_PX`).
pub fn note_size(size: f32) {
    let px = size.round();
    if (size - px).abs() > 0.01 || px < EXACT_MIN_PX as f32 || px > EXACT_MAX_PX as f32 {
        return;
    }
    let px = px as u32;
    ATLAS_SET.with(|cell| {
        let mut set = cell.borrow_mut();
        if !set.sizes.contains(&px) && set.sizes.len() + set.pending_sizes.len() < MAX_EXACT_SIZES {
            set.pending_sizes.insert(px);
        }
    });
}

/// The atlas slot that draws text of `size`, and the scale from atlas pixels
/// to drawn pixels (1.0 for an exact-size slot).
pub fn atlas_slot(size: f32) -> (usize, f32) {
    ATLAS_SET.with(|cell| {
        let set = cell.borrow();
        let px = size.round();
        if (size - px).abs() <= 0.01 {
            if let Some(i) = set.sizes.iter().position(|&s| s as f32 == px) {
                return (i + 1, 1.0);
            }
        }
        (0, size / ATLAS_BASE_PX)
    })
}

/// Number of atlases (slots × styles); atlas index = `slot * STYLE_COUNT + style`.
pub fn atlas_count() -> usize {
    ATLAS_SET.with(|cell| cell.borrow().atlases.len())
}

/// Current atlas generation — bumped on every (re)build so the renderer knows to
/// re-upload the glyph textures.
pub fn generation() -> u32 {
    ATLAS_SET.with(|cell| cell.borrow().generation)
}

/// A glyph's placement in the given style's current atlas, if baked. `GlyphInfo`
/// is `Copy`, so this hands back an owned value without holding the borrow.
pub fn glyph_info(ch: char, atlas: usize) -> Option<GlyphInfo> {
    ATLAS_SET.with(|cell| {
        let set = cell.borrow();
        set.atlases.get(atlas).and_then(|a| a.glyph(ch).copied())
    })
}

pub fn with_atlas<R>(atlas: usize, f: impl FnOnce(&Atlas) -> R) -> R {
    ATLAS_SET.with(|cell| {
        let set = cell.borrow();
        f(&set.atlases[atlas])
    })
}

fn in_base_charset(ch: char) -> bool {
    static BASE: OnceLock<BTreeSet<char>> = OnceLock::new();
    BASE.get_or_init(|| charset().into_iter().collect())
        .contains(&ch)
}

/// Build one style's glyph atlas: rasterize the base charset plus every `extra`
/// character at `ATLAS_BASE_PX` into a padded grid, recording each glyph's UV
/// rect + bearings/size. Each character is rasterized from whichever face covers
/// it (the brand face, DejaVu, or the CJK fallback).
fn build_atlas(bold: bool, mono: bool, px: f32, extra: &[char]) -> Atlas {
    let mut chars = charset();
    chars.extend_from_slice(extra);

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
        let f = face_for(ch, bold, mono);
        let glyph = f.glyph_id(ch).with_scale(px);
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
    // Square-ish grid: 16 columns for the base charset, widening toward √n as
    // extra glyphs (CJK) are added so the texture grows in both dimensions rather
    // than becoming a tall sliver that could exceed the GPU's max texture size.
    let cols = (rasters.len() as f32).sqrt().ceil() as u32;
    let cols = cols.max(16);
    let cell_w = max_w + PAD * 2;
    let cell_h = max_h + PAD * 2;
    let rows = (rasters.len() as u32).div_ceil(cols);
    let aw = (cols * cell_w).div_ceil(64) * 64;
    let ah = rows * cell_h;
    let mut rgba = vec![0u8; (aw * ah * 4) as usize];
    let mut glyphs: HashMap<char, GlyphInfo> = HashMap::new();

    for (i, r) in rasters.iter().enumerate() {
        let col = (i as u32) % cols;
        let row = (i as u32) / cols;
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
