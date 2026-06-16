//! A tiny user-agent stylesheet. Maps HTML tags onto text size/colour/spacing and
//! classifies them as skipped / container / block / inline, plus a minimal inline
//! `style=""` colour override. This is deliberately not real CSS (no selectors,
//! cascade or specificity — see the plan's deferred list); it is just enough to
//! render a readable document.

pub type Rgba = (f32, f32, f32, f32);

pub const BODY_TEXT: Rgba = (0.88, 0.88, 0.91, 1.0);
pub const HEADING: Rgba = (0.97, 0.97, 0.99, 1.0);
pub const LINK: Rgba = (0.45, 0.66, 1.0, 1.0);
pub const CODE: Rgba = (0.70, 0.85, 0.70, 1.0);

/// Resolved style for a run of text emitted by a block.
#[derive(Clone, Copy, Debug)]
pub struct TextStyle {
    pub size: f32,
    pub color: Rgba,
    /// Vertical space inserted before the block.
    pub space_before: f32,
    /// Left indent added for this block (lists, blockquotes).
    pub indent_step: f32,
}

impl TextStyle {
    pub fn body() -> Self {
        TextStyle {
            size: 15.0,
            color: BODY_TEXT,
            space_before: 0.0,
            indent_step: 0.0,
        }
    }
}

/// Tags whose entire subtree is ignored (metadata / scripting / vector markup).
pub fn is_skipped(tag: &str) -> bool {
    matches!(
        tag,
        "script" | "style" | "head" | "meta" | "link" | "title" | "noscript" | "svg" | "template"
    )
}

/// Inline tags: their text is folded into the surrounding block's run (links and
/// images are handled specially by the walker and are NOT listed here).
pub fn is_inline(tag: &str) -> bool {
    matches!(
        tag,
        "span" | "b" | "strong" | "i" | "em" | "u" | "small" | "label" | "abbr" | "cite"
            | "sub" | "sup" | "mark" | "time" | "font" | "tt" | "kbd" | "var" | "q"
    )
}

/// Block-level text tags. Returns the style for the run plus whether the text is
/// monospace-ish (rendered with the `code` colour). `None` ⇒ not a styled text
/// block (a plain container or unknown tag — recurse into children as a block).
pub fn block_style(tag: &str, base: &TextStyle) -> Option<TextStyle> {
    let s = match tag {
        "h1" => TextStyle { size: 30.0, color: HEADING, space_before: 18.0, indent_step: 0.0 },
        "h2" => TextStyle { size: 25.0, color: HEADING, space_before: 16.0, indent_step: 0.0 },
        "h3" => TextStyle { size: 21.0, color: HEADING, space_before: 14.0, indent_step: 0.0 },
        "h4" => TextStyle { size: 18.0, color: HEADING, space_before: 12.0, indent_step: 0.0 },
        "h5" => TextStyle { size: 16.0, color: HEADING, space_before: 10.0, indent_step: 0.0 },
        "h6" => TextStyle { size: 15.0, color: HEADING, space_before: 10.0, indent_step: 0.0 },
        "p" => TextStyle { size: 15.0, color: BODY_TEXT, space_before: 10.0, indent_step: 0.0 },
        "li" | "dd" => TextStyle { size: 15.0, color: BODY_TEXT, space_before: 4.0, indent_step: 24.0 },
        "dt" => TextStyle { size: 15.0, color: HEADING, space_before: 8.0, indent_step: 12.0 },
        "blockquote" => TextStyle { size: 15.0, color: (0.75, 0.78, 0.82, 1.0), space_before: 10.0, indent_step: 24.0 },
        "pre" | "code" => TextStyle { size: 14.0, color: CODE, space_before: 8.0, indent_step: 12.0 },
        _ => return None,
    };
    let _ = base;
    Some(s)
}

/// Parse an inline `style="..."` attribute for a `color:` override only.
pub fn inline_color(style_attr: &str) -> Option<Rgba> {
    for decl in style_attr.split(';') {
        let mut kv = decl.splitn(2, ':');
        let key = kv.next()?.trim().to_ascii_lowercase();
        if key == "color" {
            if let Some(val) = kv.next() {
                return parse_color(val.trim());
            }
        }
    }
    None
}

/// Parse `#rgb` / `#rrggbb` / a few named colours into RGBA.
pub fn parse_color(s: &str) -> Option<Rgba> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        let (r, g, b) = match hex.len() {
            3 => {
                let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
                let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
                let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
                (r, g, b)
            }
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                (r, g, b)
            }
            _ => return None,
        };
        return Some((r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0));
    }
    let named = match s.to_ascii_lowercase().as_str() {
        "black" => (0.0, 0.0, 0.0),
        "white" => (1.0, 1.0, 1.0),
        "red" => (0.86, 0.20, 0.18),
        "green" => (0.20, 0.66, 0.33),
        "blue" => (0.30, 0.50, 0.95),
        "gray" | "grey" => (0.55, 0.55, 0.58),
        "yellow" => (0.92, 0.86, 0.30),
        _ => return None,
    };
    Some((named.0, named.1, named.2, 1.0))
}
