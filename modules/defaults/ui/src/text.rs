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

pub fn char_to_byte_pos(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}
