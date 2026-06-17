use crate::types::*;

pub fn glyph_advance(size: f32) -> f32 {
    9.0 * (size / 8.0).max(0.125)
}

pub fn text_line_height(size: f32) -> f32 {
    10.0 * (size / 8.0).max(0.125)
}

pub fn word_wrap(line: &str, advance: f32, max_w: f32) -> Vec<String> {
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

pub fn compute_lines(text: &str, size: f32, inner_w: f32, wrap: &TextWrap) -> Vec<String> {
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
            if all_lines.is_empty() {
                all_lines.push(String::new());
            }
            all_lines
        }
        TextWrap::Newlines => text.lines().map(|l| l.to_string()).collect(),
    }
}

pub fn min_text_width(text: &str, size: f32, wrap: &TextWrap) -> f32 {
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

/// A single word placed by [`layout_words`]: its text, the char offset of its
/// first character within the source string, and its position (`x` from the
/// content-box left, `line` index from the top). Wrapping mirrors [`word_wrap`]
/// (greedy, single-space joins) but preserves char offsets so callers can map a
/// pixel position back to a char (and thus a [`TextSpan`]).
pub struct LaidWord {
    pub text: String,
    pub char_start: usize,
    pub x: f32,
    pub line: usize,
}

/// Lay `text` out into greedily-wrapped words within `inner_w`, tracking each
/// word's char offset. Used by the span draw + hit-test paths (the plain-text
/// path keeps using [`compute_lines`]). `text` is expected to be
/// whitespace-collapsed by the caller, so char offsets stay stable across lines.
pub fn layout_words(text: &str, size: f32, inner_w: f32) -> Vec<LaidWord> {
    let advance = glyph_advance(size);
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
        let word_w = (i - word_start) as f32 * advance;
        let space_w = if first_on_line { 0.0 } else { advance };

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
pub fn line_align_offsets(words: &[LaidWord], inner_w: f32, advance: f32, align: f32) -> Vec<f32> {
    if words.is_empty() {
        return Vec::new();
    }
    let n_lines = words.iter().map(|w| w.line).max().unwrap_or(0) + 1;
    let mut widths = vec![0.0f32; n_lines];
    for w in words {
        let end = w.x + w.text.chars().count() as f32 * advance;
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
