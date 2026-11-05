use crate::types::*;
use ab_glyph::{Font, FontArc, PxScale, ScaleFont};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// Weight and family of a text run (brand/BRAND.md §4): `bold` selects the
/// Medium weight, `mono` selects IBM Plex Mono instead of Space Grotesk. There
/// are no italics in the brand. A `Copy` POD so it threads cheaply through every
/// measurement helper.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FontStyle {
    pub bold: bool,
    pub mono: bool,
}

impl FontStyle {
    pub const REGULAR: FontStyle = FontStyle { bold: false, mono: false };
    pub const MEDIUM: FontStyle = FontStyle { bold: true, mono: false };
    pub const MONO: FontStyle = FontStyle { bold: false, mono: true };
    pub fn new(bold: bool, mono: bool) -> Self {
        FontStyle { bold, mono }
    }
    /// The `font` selector of a graphics text command for this style
    /// (`modules/defaults/graphics/src/font.rs::parse_style`).
    pub fn selector(self) -> Option<String> {
        match (self.bold, self.mono) {
            (false, false) => None,
            (true, false) => Some("bold".to_string()),
            (false, true) => Some("mono".to_string()),
            (true, true) => Some("mono-bold".to_string()),
        }
    }
}

/// The four embedded UI faces: Space Grotesk Regular/Medium and IBM Plex Mono
/// Regular/Medium. The graphics renderer rasterizes these *same* bytes at the
/// *same* `PxScale::from(size)` convention, so the advances we measure here
/// match the glyph positions it draws — that agreement keeps wrapped text inside
/// the box it was laid out in. MUST stay in sync with the copies in
/// `modules/defaults/graphics/assets`.
pub const FONT_TTF: &[u8] = include_bytes!("../assets/SpaceGrotesk-Regular.ttf");
pub const FONT_TTF_BOLD: &[u8] = include_bytes!("../assets/SpaceGrotesk-Medium.ttf");
pub const FONT_TTF_MONO: &[u8] = include_bytes!("../assets/IBMPlexMono-Regular.ttf");
pub const FONT_TTF_MONO_BOLD: &[u8] = include_bytes!("../assets/IBMPlexMono-Medium.ttf");
/// Fallback for what the brand faces lack (Greek, Cyrillic, most symbols):
/// DejaVu Sans, regular and bold.
pub const FONT_TTF_FALLBACK: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");
pub const FONT_TTF_FALLBACK_BOLD: &[u8] = include_bytes!("../assets/DejaVuSans-Bold.ttf");
/// CJK fallback (Droid Sans Fallback): covers Han/Hiragana/Katakana, so pages
/// with Japanese/Chinese text measure (and, on the graphics side, draw) at
/// their true widths. Single weight — used for every style.
pub const FONT_TTF_CJK: &[u8] = include_bytes!("../assets/DroidSansFallbackFull.ttf");

fn parse(bytes: &'static [u8]) -> FontArc {
    FontArc::try_from_slice(bytes).expect("embedded face is valid")
}

/// The face for `style`, lazily parsed from the embedded bytes and cached.
fn face(style: FontStyle) -> &'static FontArc {
    static FACES: [OnceLock<FontArc>; 4] = [OnceLock::new(), OnceLock::new(), OnceLock::new(), OnceLock::new()];
    let idx = (style.bold as usize) | ((style.mono as usize) << 1);
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

/// Pick the face that actually has a glyph for `ch`: the brand face for the
/// style, else DejaVu, else the CJK fallback. Rendering (graphics
/// `font::face_for`) mirrors this exactly so measured and drawn advances agree.
pub fn face_for(ch: char, style: FontStyle) -> &'static FontArc {
    let primary = face(style);
    if primary.glyph_id(ch).0 != 0 {
        return primary;
    }
    let fallback = fallback_face(style.bold);
    if fallback.glyph_id(ch).0 != 0 {
        return fallback;
    }
    if cjk_face().glyph_id(ch).0 != 0 { cjk_face() } else { primary }
}

thread_local! {
    // Per-(size, char, style) horizontal advance. Layout measures every text
    // element on the page each frame (including everything below the fold, to
    // size the scroll extent) and the draw path re-measures the visible text — so
    // the same glyph advances are queried over and over. Caching them turns each
    // query into a map lookup instead of a font cmap+hmtx read, which is the
    // difference between a heavy page rendering in a few ms vs. stalling the
    // render so long the compositor drops the window. Keyed on the size's exact
    // bit pattern (sizes recur) + the char + a 2-bit weight/family. wasm modules
    // are single-threaded so the `thread_local` is contention-free; `BTreeMap`
    // (not `HashMap`) sidesteps the `RandomState` entropy panic on
    // wasm32-unknown-unknown. The key space is tiny (a few sizes × the page's
    // distinct chars × 4 styles), so this never grows unbounded.
    static ADVANCE_CACHE: RefCell<BTreeMap<(u32, u32, u8), f32>> = RefCell::new(BTreeMap::new());
}

#[inline]
fn style_bits(style: FontStyle) -> u8 {
    ((style.bold as u8) << 1) | style.mono as u8
}

/// Horizontal advance of a single character at `size` px/em in `style`. Cached:
/// the first query for a `(size, char, style)` reads the font, every later one is
/// a map lookup. All width measurement funnels through here so the cache covers
/// the whole layout + draw path.
pub fn char_advance(ch: char, size: f32, style: FontStyle) -> f32 {
    if size <= 0.0 {
        return 0.0;
    }
    let key = (size.to_bits(), ch as u32, style_bits(style));
    if let Some(w) = ADVANCE_CACHE.with(|c| c.borrow().get(&key).copied()) {
        return w;
    }
    let sf = face_for(ch, style).as_scaled(PxScale::from(size));
    let w = sf.h_advance(sf.glyph_id(ch));
    ADVANCE_CACHE.with(|c| c.borrow_mut().insert(key, w));
    w
}

/// Total advance width of a string (newlines ignored).
pub fn text_width(s: &str, size: f32, style: FontStyle) -> f32 {
    if size <= 0.0 {
        return 0.0;
    }
    s.chars()
        .filter(|c| *c != '\n' && *c != '\r')
        .map(|c| char_advance(c, size, style))
        .sum()
}

/// Character index within `word` that the horizontal offset `dx` (from the
/// word's left edge) falls on. Used by link hit-testing; clamps to the last char.
pub fn char_index_at(word: &str, size: f32, dx: f32, style: FontStyle) -> usize {
    let mut acc = 0.0f32;
    let mut last = 0usize;
    for (i, c) in word.chars().enumerate() {
        let w = char_advance(c, size, style);
        if dx < acc + w {
            return i;
        }
        acc += w;
        last = i;
    }
    last
}

pub fn text_line_height(size: f32, style: FontStyle) -> f32 {
    if size <= 0.0 {
        return 0.0;
    }
    let sf = face(style).as_scaled(PxScale::from(size));
    (sf.height() + sf.line_gap()).ceil()
}

/// Distance from the line box's top to the baseline at `size` (where the
/// graphics renderer puts the baseline of a `draw_text` at y).
pub fn ascent(size: f32, style: FontStyle) -> f32 {
    if size <= 0.0 {
        return 0.0;
    }
    face(style).as_scaled(PxScale::from(size)).ascent()
}

pub fn word_wrap(line: &str, size: f32, max_w: f32, style: FontStyle) -> Vec<String> {
    if max_w <= 0.0 {
        return vec![line.to_string()];
    }
    let space_w = char_advance(' ', size, style);
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_w = 0.0f32;

    for word in line.split_whitespace() {
        let word_w = text_width(word, size, style);
        let lead = if current.is_empty() { 0.0 } else { space_w };
        if current.is_empty() {
            current.push_str(word);
            current_w = word_w;
        } else if current_w + lead + word_w <= max_w {
            current.push(' ');
            current.push_str(word);
            current_w += lead + word_w;
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

pub fn compute_lines(text: &str, size: f32, inner_w: f32, wrap: &TextWrap, style: FontStyle) -> Vec<String> {
    match wrap {
        TextWrap::None => text.lines().map(|l| l.to_string()).collect(),
        TextWrap::Words => {
            let mut all_lines = Vec::new();
            for explicit_line in text.lines() {
                if explicit_line.trim().is_empty() {
                    all_lines.push(String::new());
                } else {
                    all_lines.extend(word_wrap(explicit_line, size, inner_w, style));
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

pub fn min_text_width(text: &str, size: f32, wrap: &TextWrap, style: FontStyle) -> f32 {
    match wrap {
        TextWrap::Words => text
            .split_whitespace()
            .map(|w| text_width(w, size, style))
            .fold(0.0f32, f32::max),
        TextWrap::None | TextWrap::Newlines => text
            .lines()
            .map(|l| text_width(l, size, style))
            .fold(0.0f32, f32::max),
    }
}

/// A single word placed by [`layout_words`]: its text, the char offset of its
/// first character within the source string, and its position (`x` from the
/// content-box left, `line` index from the top). Wrapping mirrors [`word_wrap`]
/// (greedy, single-space joins) but preserves char offsets so callers can map a
/// pixel position back to a char (and thus a [`TextSpan`]).
pub struct LaidWord {
    pub text: String,
    pub char_start: usize,
    pub x: f32,
    /// Advance width of the word, measured with each char's own style — so
    /// callers never re-measure (and never need the per-char styles again).
    pub width: f32,
    pub line: usize,
}

/// Lay `text` out into greedily-wrapped words within `inner_w`, tracking each
/// word's char offset. Used by the span draw + hit-test paths (the plain-text
/// path keeps using [`compute_lines`]). `text` is expected to be
/// whitespace-collapsed by the caller, so char offsets stay stable across lines.
///
/// `style_at` yields the [`FontStyle`] of the char at a given char index, so
/// words mixing regular/medium/mono runs measure correctly; plain callers pass a closure
/// returning the element's base style.
pub fn layout_words(
    text: &str,
    size: f32,
    inner_w: f32,
    style_at: impl Fn(usize) -> FontStyle,
) -> Vec<LaidWord> {
    // Inter-word spacing uses the regular space advance; the difference between
    // faces for U+0020 is negligible and not worth a per-gap style lookup.
    let space_adv = char_advance(' ', size, FontStyle::REGULAR);
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<LaidWord> = Vec::new();
    let mut line = 0usize;
    let mut cursor_x = 0.0f32;
    let mut first_on_line = true;

    let mut i = 0;
    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let word_start = i;
        while i < chars.len() && !chars[i].is_whitespace() {
            i += 1;
        }
        let word: String = chars[word_start..i].iter().collect();
        let word_w: f32 = (word_start..i)
            .map(|ci| char_advance(chars[ci], size, style_at(ci)))
            .sum();
        let space_w = if first_on_line { 0.0 } else { space_adv };

        if !first_on_line && inner_w > 0.0 && cursor_x + space_w + word_w > inner_w {
            line += 1;
            cursor_x = 0.0;
            first_on_line = true;
        }

        let x = if first_on_line { 0.0 } else { cursor_x + space_w };
        out.push(LaidWord {
            text: word,
            char_start: word_start,
            x,
            width: word_w,
            line,
        });
        cursor_x = x + word_w;
        first_on_line = false;
    }
    out
}

/// Horizontal offset to apply to a line of pixel width `line_w` inside a content
/// box of width `inner_w` for alignment factor `align` (0 left, 0.5 centre, 1
/// right). Never negative (overflowing lines stay left-anchored).
pub fn align_offset(inner_w: f32, line_w: f32, align: f32) -> f32 {
    if align <= 0.0 {
        return 0.0;
    }
    ((inner_w - line_w) * align).max(0.0)
}

/// Per-line alignment offsets for a laid-out span paragraph. Index by `LaidWord::line`.
pub fn line_align_offsets(words: &[LaidWord], inner_w: f32, align: f32) -> Vec<f32> {
    if words.is_empty() {
        return Vec::new();
    }
    let n_lines = words.iter().map(|w| w.line).max().unwrap_or(0) + 1;
    let mut widths = vec![0.0f32; n_lines];
    for w in words {
        let end = w.x + w.width;
        if end > widths[w.line] {
            widths[w.line] = end;
        }
    }
    widths
        .into_iter()
        .map(|lw| align_offset(inner_w, lw, align))
        .collect()
}

/// Colour for char `idx`, falling back to `default` when no span covers it.
pub fn span_color_at(spans: &[TextSpan], idx: usize, default: (f32, f32, f32, f32)) -> (f32, f32, f32, f32) {
    for s in spans {
        if idx >= s.start as usize && idx < s.end as usize {
            return s.color;
        }
    }
    default
}

/// [`FontStyle`] for char `idx`: the covering span's own weight/family, else the
/// element's `base` style. A span carries the already-resolved weight/family
/// (inheriting the block's base when the inline tag added none), so this is a
/// plain override — mirroring [`span_color_at`].
pub fn span_style_at(spans: &[TextSpan], idx: usize, base: FontStyle) -> FontStyle {
    for s in spans {
        if idx >= s.start as usize && idx < s.end as usize {
            return FontStyle { bold: s.bold, mono: s.mono };
        }
    }
    base
}

/// `href` for char `idx`, if a link span covers it.
pub fn span_href_at(spans: &[TextSpan], idx: usize) -> Option<String> {
    for s in spans {
        if idx >= s.start as usize && idx < s.end as usize {
            if let Some(h) = &s.href {
                return Some(h.clone());
            }
        }
    }
    None
}

pub fn char_to_byte_pos(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}
