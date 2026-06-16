//! Minimal URL handling for the browser. HTTP-only (the broker has no TLS), so
//! we keep just what a navigation needs: a `(host, path)` pair plus relative →
//! absolute resolution against the current page.

/// A parsed HTTP location. Port is ignored — the broker always uses port 80.
#[derive(Clone, Debug)]
pub struct Location {
    pub host: String,
    pub path: String,
}

impl Location {
    /// Reassemble a display URL.
    pub fn to_url(&self) -> String {
        format!("http://{}{}", self.host, self.path)
    }
}

/// Parse a user- or link-supplied URL into `(host, path)`. A missing scheme is
/// assumed to be `http://`. `https://` is accepted syntactically (so links don't
/// silently vanish) but will fail to fetch over the plain-HTTP broker. Returns
/// `None` if no host can be extracted.
pub fn parse(input: &str) -> Option<Location> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Strip scheme.
    let rest = if let Some(r) = trimmed.strip_prefix("http://") {
        r
    } else if let Some(r) = trimmed.strip_prefix("https://") {
        r
    } else if let Some(r) = trimmed.strip_prefix("//") {
        r
    } else {
        trimmed
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
    })
}

/// Resolve `href` against the current page `(base host, base path)` into an
/// absolute `(host, path)`. Handles absolute URLs, protocol-relative `//host`,
/// root-relative `/path`, and document-relative `path` links.
pub fn resolve(base_host: &str, base_path: &str, href: &str) -> Option<Location> {
    let href = strip_fragment(href.trim());
    if href.is_empty() {
        // Pure fragment / empty → same page.
        return Some(Location {
            host: base_host.to_string(),
            path: base_path.to_string(),
        });
    }

    // Absolute or protocol-relative: parse directly.
    if href.starts_with("http://") || href.starts_with("https://") || href.starts_with("//") {
        return parse(href);
    }

    // Root-relative.
    if let Some(stripped) = href.strip_prefix('/') {
        return Some(Location {
            host: base_host.to_string(),
            path: format!("/{stripped}"),
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
