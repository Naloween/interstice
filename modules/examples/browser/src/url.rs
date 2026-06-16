//! Minimal URL handling for the browser. The broker terminates TLS host-side, so
//! we track the scheme (`tls`) alongside the `(host, path)` a navigation needs,
//! plus relative → absolute resolution against the current page.

/// A parsed location. Port is ignored — the broker uses 80 for http, 443 for
/// https; `tls` records which scheme this URL used.
#[derive(Clone, Debug)]
pub struct Location {
    pub host: String,
    pub path: String,
    pub tls: bool,
}

impl Location {
    /// Reassemble a display URL.
    pub fn to_url(&self) -> String {
        let scheme = if self.tls { "https" } else { "http" };
        format!("{scheme}://{}{}", self.host, self.path)
    }
}

/// Parse a user- or link-supplied URL into a [`Location`]. `https://` selects TLS;
/// a missing scheme is assumed to be `http://`. A protocol-relative `//host` keeps
/// `default_tls`. Returns `None` if no host can be extracted.
pub fn parse(input: &str) -> Option<Location> {
    parse_with_scheme(input, false)
}

/// Like [`parse`], but a scheme-less or protocol-relative input inherits
/// `default_tls` (used when resolving a relative link from an https page).
fn parse_with_scheme(input: &str, default_tls: bool) -> Option<Location> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Strip scheme, remembering whether it selected TLS.
    let (rest, tls) = if let Some(r) = trimmed.strip_prefix("http://") {
        (r, false)
    } else if let Some(r) = trimmed.strip_prefix("https://") {
        (r, true)
    } else if let Some(r) = trimmed.strip_prefix("//") {
        (r, default_tls)
    } else {
        (trimmed, default_tls)
    };

    // Split host[:port] from the path. Drop any fragment.
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let host = authority.split(':').next().unwrap_or(authority).trim();
    if host.is_empty() || !is_valid_host(host) {
        return None;
    }
    let path = strip_fragment(path);
    let path = if path.is_empty() { "/".to_string() } else { path.to_string() };

    Some(Location {
        host: host.to_string(),
        path,
        tls,
    })
}

/// Resolve `href` against the current page `(base host, base path)` into an
/// absolute `(host, path)`. Handles absolute URLs, protocol-relative `//host`,
/// root-relative `/path`, and document-relative `path` links.
pub fn resolve(base_host: &str, base_path: &str, base_tls: bool, href: &str) -> Option<Location> {
    let href = strip_fragment(href.trim());
    if href.is_empty() {
        // Pure fragment / empty → same page.
        return Some(Location {
            host: base_host.to_string(),
            path: base_path.to_string(),
            tls: base_tls,
        });
    }

    // Absolute or protocol-relative: parse directly, inheriting the page's scheme
    // for a protocol-relative `//host`.
    if href.starts_with("http://") || href.starts_with("https://") || href.starts_with("//") {
        return parse_with_scheme(href, base_tls);
    }

    // Root-relative (same host + scheme).
    if let Some(stripped) = href.strip_prefix('/') {
        return Some(Location {
            host: base_host.to_string(),
            path: format!("/{stripped}"),
            tls: base_tls,
        });
    }

    // Document-relative: resolve against the base path's directory.
    let dir = match base_path.rfind('/') {
        Some(i) => &base_path[..=i],
        None => "/",
    };
    Some(Location {
        host: base_host.to_string(),
        path: normalize(&format!("{dir}{href}")),
        tls: base_tls,
    })
}

fn strip_fragment(s: &str) -> &str {
    s.split('#').next().unwrap_or(s)
}

/// Reject hostnames containing characters that can't appear in a DNS name, so a
/// garbled URL (or a stray input event) can never reach the resolver.
fn is_valid_host(host: &str) -> bool {
    host.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
}

/// Collapse `.` and `..` path segments.
fn normalize(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    let mut result = String::from("/");
    result.push_str(&out.join("/"));
    result
}
