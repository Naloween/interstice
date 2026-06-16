//! HTML → flat render-block translation. We parse with `tl`, walk the DOM, and
//! flatten it into a linear list of [`Block`]s (the document flow). Inline runs
//! are concatenated per block; links and images become their own blocks so they
//! can be hit-tested / fetched individually (see the plan: v1 keeps inline flow
//! simple). The caller turns blocks into `UiElement`s under the viewport.

use crate::style::{self, Rgba, TextStyle};

/// One laid-out piece of the document, in reading order.
pub enum Block {
    /// A run of text. `href` set ⇒ it is a link (link-coloured, clickable).
    Text {
        text: String,
        size: f32,
        color: Rgba,
        indent: f32,
        href: Option<String>,
    },
    /// An `<img>` to fetch + decode. `url` is as authored (resolved later).
    Image { url: String },
    /// Vertical whitespace (paragraph spacing, `<br>`, `<hr>`).
    Space { height: f32 },
}

struct State {
    blocks: Vec<Block>,
    buf: String,
    style: TextStyle,
    indent: f32,
}

/// Parse a document and return its blocks. Invalid HTML yields an empty list.
pub fn parse_html(html: &str) -> Vec<Block> {
    let dom = match tl::parse(html, tl::ParserOptions::default()) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    let parser = dom.parser();
    let mut st = State {
        blocks: Vec::new(),
        buf: String::new(),
        style: TextStyle::body(),
        indent: 0.0,
    };
    for handle in dom.children() {
        walk(parser, *handle, &mut st);
    }
    flush(&mut st);
    st.blocks
}

fn walk(parser: &tl::Parser, handle: tl::NodeHandle, st: &mut State) {
    let node = match handle.get(parser) {
        Some(n) => n,
        None => return,
    };
    match node {
        tl::Node::Raw(bytes) => {
            st.buf.push_str(&decode_entities(&bytes.as_utf8_str()));
        }
        tl::Node::Comment(_) => {}
        tl::Node::Tag(tag) => {
            let name = tag.name().as_utf8_str().to_ascii_lowercase();
            tag_node(parser, &name, tag, st);
        }
    }
}

fn tag_node(parser: &tl::Parser, name: &str, tag: &tl::HTMLTag, st: &mut State) {
    if style::is_skipped(name) {
        return;
    }

    // Inline `style="color: …"` override (the only CSS we honour — see plan).
    let color_override = tag
        .attributes()
        .get("style")
        .flatten()
        .and_then(|b| style::inline_color(&b.as_utf8_str()));

    match name {
        "a" => {
            flush(st);
            let href = tag
                .attributes()
                .get("href")
                .flatten()
                .map(|b| b.as_utf8_str().to_string());
            let text = collapse_ws(&decode_entities(&tag.inner_text(parser)));
            if !text.is_empty() {
                st.blocks.push(Block::Text {
                    text,
                    size: st.style.size,
                    color: style::LINK,
                    indent: st.indent,
                    href,
                });
            }
            return;
        }
        "img" => {
            flush(st);
            if let Some(Some(src)) = tag.attributes().get("src") {
                let url = src.as_utf8_str().to_string();
                if !url.is_empty() {
                    st.blocks.push(Block::Image { url });
                }
            }
            return;
        }
        "br" => {
            flush(st);
            st.blocks.push(Block::Space { height: 6.0 });
            return;
        }
        "hr" => {
            flush(st);
            st.blocks.push(Block::Space { height: 14.0 });
            return;
        }
        _ => {}
    }

    // Inline formatting: fold the children's text into the current run, unless a
    // colour override is present — then emit the span as its own coloured run.
    if style::is_inline(name) {
        if let Some(color) = color_override {
            flush(st);
            let text = collapse_ws(&decode_entities(&tag.inner_text(parser)));
            if !text.is_empty() {
                st.blocks.push(Block::Text {
                    text,
                    size: st.style.size,
                    color,
                    indent: st.indent,
                    href: None,
                });
            }
        } else {
            walk_children(parser, tag, st);
        }
        return;
    }

    // Block-level text: start a fresh run with this tag's style.
    if let Some(bs) = style::block_style(name, &st.style) {
        flush(st);
        if bs.space_before > 0.0 {
            st.blocks.push(Block::Space {
                height: bs.space_before,
            });
        }
        let saved_style = st.style;
        let saved_indent = st.indent;
        st.style = bs;
        if let Some(color) = color_override {
            st.style.color = color;
        }
        st.indent += bs.indent_step;
        if name == "li" {
            st.buf.push_str("• ");
        }
        walk_children(parser, tag, st);
        flush(st);
        st.style = saved_style;
        st.indent = saved_indent;
        return;
    }

    // Container or unknown tag: descend without emitting a run boundary.
    walk_children(parser, tag, st);
}

fn walk_children(parser: &tl::Parser, tag: &tl::HTMLTag, st: &mut State) {
    // Copy handles out first (NodeHandle is Copy) so the immutable borrow of
    // `tag` doesn't overlap the recursive mutation of `st`.
    let kids: Vec<tl::NodeHandle> = tag.children().top().iter().copied().collect();
    for handle in kids {
        walk(parser, handle, st);
    }
}

/// Emit the pending text buffer as a Text block, collapsing whitespace.
fn flush(st: &mut State) {
    let collapsed = collapse_ws(&st.buf);
    st.buf.clear();
    if !collapsed.is_empty() {
        st.blocks.push(Block::Text {
            text: collapsed,
            size: st.style.size,
            color: st.style.color,
            indent: st.indent,
            href: None,
        });
    }
}

/// Collapse all runs of ASCII/Unicode whitespace into single spaces and trim.
fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Decode the handful of HTML entities common in plain documents.
fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&mdash;", "—")
        .replace("&ndash;", "–")
        .replace("&hellip;", "…")
}
