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
    /// Base weight/slant of the block's text (headings default to bold).
    pub bold: bool,
    pub italic: bool,
}

impl TextStyle {
    pub fn body() -> Self {
        TextStyle {
            size: 15.0,
            color: BODY_TEXT,
            space_before: 0.0,
            indent_step: 0.0,
            bold: false,
            italic: false,
        }
    }
}

/// Whether an inline tag implies bold (`b`/`strong`) or italic (`i`/`em`/
/// `cite`/`var`) by the UA default. Author CSS `font-weight`/`font-style` can
/// still override these in the walker.
pub fn inline_bold(tag: &str) -> bool {
    matches!(tag, "b" | "strong")
}
pub fn inline_italic(tag: &str) -> bool {
    matches!(tag, "i" | "em" | "cite" | "var")
}

/// Tags whose entire subtree is ignored (metadata / scripting). `svg` is *not*
/// here: inline SVG is rasterized as an image (see [`crate::html`]'s `tag_node`).
pub fn is_skipped(tag: &str) -> bool {
    matches!(
        tag,
        "script" | "style" | "head" | "meta" | "link" | "title" | "noscript" | "template"
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
    // Start from the base so unspecified fields (bold/italic) default to normal;
    // headings then flip `bold`.
    let bold_heading = TextStyle { bold: true, ..*base };
    let s = match tag {
        "h1" => TextStyle { size: 30.0, color: HEADING, space_before: 18.0, indent_step: 0.0, ..bold_heading },
        "h2" => TextStyle { size: 25.0, color: HEADING, space_before: 16.0, indent_step: 0.0, ..bold_heading },
        "h3" => TextStyle { size: 21.0, color: HEADING, space_before: 14.0, indent_step: 0.0, ..bold_heading },
        "h4" => TextStyle { size: 18.0, color: HEADING, space_before: 12.0, indent_step: 0.0, ..bold_heading },
        "h5" => TextStyle { size: 16.0, color: HEADING, space_before: 10.0, indent_step: 0.0, ..bold_heading },
        "h6" => TextStyle { size: 15.0, color: HEADING, space_before: 10.0, indent_step: 0.0, ..bold_heading },
        "p" => TextStyle { size: 15.0, color: BODY_TEXT, space_before: 10.0, indent_step: 0.0, bold: false, italic: false },
        "li" | "dd" => TextStyle { size: 15.0, color: BODY_TEXT, space_before: 4.0, indent_step: 24.0, bold: false, italic: false },
        "dt" => TextStyle { size: 15.0, color: HEADING, space_before: 8.0, indent_step: 12.0, bold: false, italic: false },
        "blockquote" => TextStyle { size: 15.0, color: (0.75, 0.78, 0.82, 1.0), space_before: 10.0, indent_step: 24.0, bold: false, italic: false },
        "pre" | "code" => TextStyle { size: 14.0, color: CODE, space_before: 8.0, indent_step: 12.0, bold: false, italic: false },
        _ => return None,
    };
    Some(s)
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
