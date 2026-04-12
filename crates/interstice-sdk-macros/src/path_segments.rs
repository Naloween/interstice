//! Dotted path splitting aligned with `on = "…"` subscription strings:
//! split on `.`, trim segments, reject empty segments (e.g. `a..b`).

use proc_macro2::Span;
use syn::Error;

pub(crate) fn segments_from_dotted_str(content: &str, span: Span) -> Result<Vec<String>, Error> {
    let mut out = Vec::new();
    for part in content.split('.') {
        let t = part.trim();
        if t.is_empty() {
            return Err(Error::new(
                span,
                "dotted path must not contain empty segments between dots",
            ));
        }
        out.push(t.to_string());
    }
    if out.is_empty() {
        return Err(Error::new(span, "path must not be empty"));
    }
    Ok(out)
}
