//! Blitz embedder hooks. Blitz talks to its host through three provider traits:
//!
//! - [`NetProvider`]: sub-resource fetches (stylesheets, images, fonts…). We
//!   cannot block or spawn, so a fetch is parked in [`NET_QUEUE`]; `on_frame`
//!   hands it to the shared `network` broker and `on_http` completes it.
//! - [`NavigationProvider`]: link clicks / form submissions → [`NAV_QUEUE`],
//!   drained by `on_frame` into a tab navigation.
//! - [`ShellProvider`]: redraw requests, cursor shape, clipboard.
//!
//! The traits require `Send + Sync`; a wasm module is single-threaded, so plain
//! mutex-guarded statics are enough.

use blitz_traits::navigation::{NavigationOptions, NavigationProvider};
use blitz_traits::net::{NetHandler, NetProvider, Request};
use blitz_traits::shell::{ClipboardError, ShellProvider};
use bytes::Bytes;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// A sub-resource fetch requested by a document, waiting to be issued.
pub struct PendingFetch {
    pub doc_id: usize,
    pub url: url::Url,
    pub handler: Box<dyn NetHandler>,
}

pub static NET_QUEUE: Mutex<Vec<PendingFetch>> = Mutex::new(Vec::new());
pub static NAV_QUEUE: Mutex<Vec<NavigationOptions>> = Mutex::new(Vec::new());
pub static REDRAW: AtomicBool = AtomicBool::new(true);
pub static CURSOR: Mutex<Option<cursor_icon::CursorIcon>> = Mutex::new(None);
pub static CLIPBOARD: Mutex<String> = Mutex::new(String::new());

pub struct Net;

impl NetProvider for Net {
    fn fetch(&self, doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        let url = request.url;
        match url.scheme() {
            // Inline payloads resolve immediately; the document picks the result
            // up from its message channel on the next `resolve`.
            "data" => {
                let bytes = decode_data_url(url.as_str()).unwrap_or_default();
                handler.bytes(url.to_string(), Bytes::from(bytes));
            }
            "http" | "https" => NET_QUEUE.lock().unwrap().push(PendingFetch { doc_id, url, handler }),
            // Unsupported scheme: complete with an empty body so a critical
            // resource (a stylesheet) never blocks the first paint.
            _ => handler.bytes(url.to_string(), Bytes::new()),
        }
    }
}

pub struct Nav;

impl NavigationProvider for Nav {
    fn navigate_to(&self, options: NavigationOptions) {
        NAV_QUEUE.lock().unwrap().push(options);
    }
}

pub struct Shell;

impl ShellProvider for Shell {
    fn request_redraw(&self) {
        REDRAW.store(true, Ordering::Relaxed);
    }
    fn set_cursor(&self, icon: Option<cursor_icon::CursorIcon>) {
        *CURSOR.lock().unwrap() = icon;
    }
    fn get_clipboard_text(&self) -> Result<String, ClipboardError> {
        Ok(CLIPBOARD.lock().unwrap().clone())
    }
    fn set_clipboard_text(&self, text: String) -> Result<(), ClipboardError> {
        *CLIPBOARD.lock().unwrap() = text;
        Ok(())
    }
}

/// Shell for the browser chrome document: same as [`Shell`] but with its own
/// redraw flag so chrome hover changes don't repaint the page.
pub struct ChromeShell;

pub static CHROME_REDRAW: AtomicBool = AtomicBool::new(true);

impl ShellProvider for ChromeShell {
    fn request_redraw(&self) {
        CHROME_REDRAW.store(true, Ordering::Relaxed);
    }
    fn get_clipboard_text(&self) -> Result<String, ClipboardError> {
        Ok(CLIPBOARD.lock().unwrap().clone())
    }
    fn set_clipboard_text(&self, text: String) -> Result<(), ClipboardError> {
        *CLIPBOARD.lock().unwrap() = text;
        Ok(())
    }
}

pub fn take_chrome_redraw() -> bool {
    CHROME_REDRAW.swap(false, Ordering::Relaxed)
}

/// Standard base64 encoding (for `data:` URLs built from fetched bytes).
pub fn base64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

pub fn take_redraw() -> bool {
    REDRAW.swap(false, Ordering::Relaxed)
}

/// Decode a `data:[<mime>][;base64],<payload>` URL.
pub fn decode_data_url(u: &str) -> Option<Vec<u8>> {
    let rest = u.strip_prefix("data:")?;
    let comma = rest.find(',')?;
    let (meta, payload) = (&rest[..comma], &rest[comma + 1..]);
    let raw = percent_decode(payload);
    if meta.ends_with(";base64") {
        base64_decode(&raw)
    } else {
        Some(raw)
    }
}

fn percent_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

fn hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

pub fn base64_decode(input: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits = 0;
    for &c in input {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return None,
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}
