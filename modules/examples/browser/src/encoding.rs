//! Response-body text decoding.
//!
//! The network layer hands us raw bytes; the HTTP `Content-Type` charset isn't
//! surfaced, so we sniff: a body that is valid UTF-8 is decoded as UTF-8 (the
//! modern default), and anything else falls back to **Windows-1252** — the
//! legacy encoding browsers use when a page is labelled `ISO-8859-1` or carries
//! no usable label (e.g. google.com serves `charset=ISO-8859-1`, so its
//! "Français" is a lone `0xE7` byte that `from_utf8_lossy` would mangle into a
//! replacement char). Windows-1252 is a superset of ISO-8859-1 over the
//! printable range, so it is the correct single fallback.

/// Windows-1252 mappings for the `0x80..=0x9F` range, where it differs from
/// ISO-8859-1 (which leaves those as C1 controls). Every other byte maps to the
/// identical Unicode code point.
const CP1252_HIGH: [char; 32] = [
    '\u{20AC}', '\u{0081}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{008D}', '\u{017D}', '\u{008F}',
    '\u{0090}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}', '\u{009D}', '\u{017E}', '\u{0178}',
];

/// Decode a response body to a `String`, preferring UTF-8 and falling back to
/// Windows-1252 for non-UTF-8 bytes.
pub fn decode_body(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes
            .iter()
            .map(|&b| match b {
                0x80..=0x9F => CP1252_HIGH[(b - 0x80) as usize],
                _ => b as char,
            })
            .collect(),
    }
}
