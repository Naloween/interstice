//! Browser logic over the tables in [`crate::model`].
//!
//! A reducer loads the small rows it works on (`Session`, `Ui`, all `Tab`s)
//! into a [`Browser`], runs browser logic on them, and writes back whatever
//! changed. Larger or append-only tables (`Document`, `DocEvent`, `Request`,
//! `Resource`) are read and written directly through a [`Store`]; the engine's
//! read-your-writes makes those writes visible to later reads in the same
//! reducer run.
//!
//! Parsed documents live in the instance's [`Caches`]. They are derived data:
//! [`Browser::ensure_active_doc`] rebuilds a missing one from its `Document`
//! row (HTML + cached sub-resources) and replays its `DocEvent`s, and catches
//! a stale one up from the events another instance applied. Side effects of
//! input (link navigations) only happen when an event is applied live, never
//! during a replay.

use crate::cache::{Caches, ChromeCache, ChromeView, DocCache};
use crate::chrome::{self, ChromeAction, ChromeHandler, TabView, CHROME_H};
use crate::js::{self, Js};
use crate::keys::{self, Mods};
use crate::model::*;
use crate::pages;
use crate::providers::{self, PendingFetch};
use blitz_dom::{BaseDocument, Document as _, DocumentConfig, EventDriver, EventHandler};
use anyrender::recording::RenderCommand;
use blitz_html::HtmlDocument;
use blitz_traits::events::{
    BlitzPointerEvent, BlitzPointerId, BlitzWheelDelta, BlitzWheelEvent, DomEvent, DomEventData, EventState,
    MouseEventButton, MouseEventButtons, Point, PointerCoords, PointerDetails, UiEvent,
};
use blitz_traits::shell::{ColorScheme, Viewport};
use bytes::Bytes;
use keyboard_types::{Key, NamedKey};
use std::sync::Arc;

/// Table access for the parts of the state not held in [`Browser`].
pub trait Store {
    fn document(&self, id: u64) -> Option<Document>;
    fn insert_document(&self, doc: Document);
    /// Delete a document and its event log.
    fn drop_document(&self, id: u64);
    /// A document's events, in `seq` order.
    fn doc_events(&self, doc: u64) -> Vec<DocEvent>;
    /// Append an event; returns its `seq`.
    fn push_doc_event(&self, ev: DocEvent) -> u64;
    /// Delete a document's event log (compaction).
    fn clear_doc_events(&self, doc: u64);
    fn resource(&self, url: &str) -> Option<Vec<u8>>;
    fn has_resource(&self, url: &str) -> bool;
    fn insert_resource(&self, url: &str, body: Vec<u8>, stamp: u64);
    /// Evict the oldest resource; returns its size.
    fn evict_oldest_resource(&self) -> Option<u64>;
    fn request_pending_for(&self, url: &str) -> bool;
    fn insert_request(&self, req: Request);
    fn delete_request(&self, req_id: u64);
    /// `localStorage` contents of an origin.
    fn web_storage(&self, origin: &str) -> Vec<(String, String)>;
    /// Set (`Some`) or remove (`None`) one `localStorage` item.
    fn set_web_storage(&self, origin: &str, name: &str, value: Option<String>);
}

/// How a completed navigation affects the session history.
#[derive(Clone, Copy, Debug)]
pub enum NavMode {
    Push,
    Replace,
    Traverse(usize),
}

impl NavMode {
    fn encode(self) -> (u8, u32) {
        match self {
            NavMode::Push => (NAV_PUSH, 0),
            NavMode::Replace => (NAV_REPLACE, 0),
            NavMode::Traverse(i) => (NAV_TRAVERSE, i as u32),
        }
    }
    fn decode(mode: u8, index: u32) -> Self {
        match mode {
            NAV_REPLACE => NavMode::Replace,
            NAV_TRAVERSE => NavMode::Traverse(index as usize),
            _ => NavMode::Push,
        }
    }
}

const REGION_CHROME: u8 = 1;
const REGION_CONTENT: u8 = 2;

/// Stamp shown by a blank content texture (no document yet).
const BLANK_STAMP: u64 = 1;

pub fn content_height(h: u32) -> u32 {
    h.saturating_sub(CHROME_H).max(1)
}

fn pointer(x: f32, y: f32, button: MouseEventButton, buttons: MouseEventButtons, mods: Mods) -> BlitzPointerEvent {
    BlitzPointerEvent {
        id: BlitzPointerId::Mouse,
        is_primary: true,
        coords: PointerCoords { page_x: x, page_y: y, screen_x: x, screen_y: y, client_x: x, client_y: y },
        button,
        buttons,
        mods: mods.to_modifiers(),
        details: PointerDetails::default(),
        element: Point::default(),
        active_pointers: Default::default(),
    }
}

/// Blitz hit-tests with `page_*` = document coordinates: the viewport
/// position plus the scroll offset (`client_*` stays the viewport position).
fn scrolled(mut p: BlitzPointerEvent, doc: &blitz_dom::BaseDocument) -> BlitzPointerEvent {
    let s = doc.viewport_scroll();
    p.coords.page_x += s.x as f32;
    p.coords.page_y += s.y as f32;
    p
}

fn button_of(id: u8) -> MouseEventButton {
    match id {
        1 => MouseEventButton::Secondary,
        2 => MouseEventButton::Auxiliary,
        _ => MouseEventButton::Main,
    }
}

/// Turn address-bar input into a URL (or a search).
pub fn fixup_url(input: &str) -> String {
    let s = input.trim();
    if s.is_empty() {
        return "about:newtab".into();
    }
    if s.starts_with("about:") || s.starts_with("data:") || s.contains("://") {
        return s.to_string();
    }
    let host_part = s.split(['/', '?', '#']).next().unwrap_or("");
    let looks_like_host = !s.contains(' ')
        && (host_part.contains('.') || host_part == "localhost" || host_part.contains(':'))
        && !host_part.starts_with('.')
        && !host_part.ends_with('.');
    if looks_like_host { format!("https://{s}") } else { pages::search_url(s) }
}

fn sniff_mime(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(b"\x89PNG") {
        Some("image/png")
    } else if b.starts_with(&[0xff, 0xd8]) {
        Some("image/jpeg")
    } else if b.starts_with(b"GIF8") {
        Some("image/gif")
    } else if b.len() > 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("image/webp")
    } else if b.starts_with(&[0, 0, 1, 0]) {
        Some("image/x-icon")
    } else if b.starts_with(b"BM") {
        Some("image/bmp")
    } else {
        let head = String::from_utf8_lossy(&b[..b.len().min(512)]).to_ascii_lowercase();
        if head.contains("<svg") && !head.contains("<html") { Some("image/svg+xml") } else { None }
    }
}

fn looks_like_html(b: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&b[..b.len().min(1024)]).to_ascii_lowercase();
    ["<html", "<!doctype", "<head", "<body", "<div", "<p", "<a ", "<title", "<meta", "<script"]
        .iter()
        .any(|t| head.contains(t))
}

pub fn parse_url(url: &str) -> Option<(String, String, bool)> {
    let u = url::Url::parse(url).ok()?;
    let tls = match u.scheme() {
        "https" => true,
        "http" => false,
        _ => return None,
    };
    let host = match u.port() {
        Some(p) => format!("{}:{p}", u.host_str()?),
        None => u.host_str()?.to_string(),
    };
    let mut path = u.path().to_string();
    if let Some(q) = u.query() {
        path.push('?');
        path.push_str(q);
    }
    Some((host, path, tls))
}

pub struct Browser<'a> {
    pub session: Session,
    pub ui: Ui,
    /// Sorted by id (= opening order).
    pub tabs: Vec<Tab>,
    db: &'a dyn Store,
    caches: &'a mut Caches,
}

impl<'a> Browser<'a> {
    pub fn new(db: &'a dyn Store, caches: &'a mut Caches, session: Session, ui: Ui, mut tabs: Vec<Tab>) -> Self {
        tabs.sort_by_key(|t| t.id);
        Browser { session, ui, tabs, db, caches }
    }

    pub fn into_rows(self) -> (Session, Ui, Vec<Tab>) {
        (self.session, self.ui, self.tabs)
    }

    fn active(&self) -> usize {
        self.tabs.iter().position(|t| t.id == self.session.active_tab).unwrap_or(0)
    }

    fn tab_index(&self, id: u64) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    fn mods(&self) -> Mods {
        Mods::from_bits(self.ui.mods)
    }

    fn buttons(&self) -> MouseEventButtons {
        MouseEventButtons::from_bits_truncate(self.ui.buttons)
    }

    fn content_size(&self) -> (u32, u32) {
        (self.ui.viewport.0.max(1), content_height(self.ui.viewport.1))
    }

    fn doc_config(&self, url: &str) -> DocumentConfig {
        let (w, h) = self.content_size();
        DocumentConfig {
            viewport: Some(Viewport::new(w, h, 1.0, ColorScheme::Light)),
            base_url: Some(url.to_string()),
            net_provider: Some(Arc::new(providers::Net)),
            navigation_provider: Some(Arc::new(providers::Nav)),
            shell_provider: Some(Arc::new(providers::Shell)),
            html_parser_provider: Some(Arc::new(blitz_html::HtmlProvider)),
            font_ctx: Some(crate::fonts::font_context()),
            ..Default::default()
        }
    }

    fn active_cache(&mut self) -> Option<&mut DocCache> {
        let doc = self.tabs.get(self.active())?.doc;
        self.caches.docs.get_mut(&doc)
    }

    // ── Tabs ─────────────────────────────────────────────────────────────

    pub fn new_tab(&mut self, url: &str) {
        let id = self.session.next_tab;
        self.session.next_tab += 1;
        self.tabs.push(Tab {
            id,
            url: String::new(),
            title: String::new(),
            favicon: None,
            history: Vec::new(),
            hist_pos: 0,
            loading: None,
            doc: 0,
            doc_seq: 0,
            doc_parsed: false,
        });
        self.session.active_tab = id;
        let idx = self.tabs.len() - 1;
        self.navigate(idx, url, NavMode::Push);
        if url.starts_with("about:newtab") || url.starts_with("about:home") {
            self.focus_url();
        } else {
            self.blur_url();
        }
    }

    pub fn close_tab(&mut self, idx: usize) {
        if idx >= self.tabs.len() {
            return;
        }
        let active = self.active();
        let tab = self.tabs.remove(idx);
        if let Some(r) = tab.loading {
            self.db.delete_request(r);
        }
        if tab.doc != 0 {
            self.db.drop_document(tab.doc);
            self.caches.docs.remove(&tab.doc);
        }
        if self.tabs.is_empty() {
            self.new_tab("about:newtab");
            return;
        }
        let next = if active > idx || active >= self.tabs.len() { active.saturating_sub(1) } else { active };
        self.select_tab(next.min(self.tabs.len() - 1));
    }

    pub fn select_tab(&mut self, idx: usize) {
        let Some(t) = self.tabs.get(idx) else { return };
        self.session.active_tab = t.id;
        if t.url.starts_with("about:newtab") {
            self.focus_url();
        } else {
            self.blur_url();
        }
    }

    /// After a restart: tabs came back from their stateful table, documents
    /// (ephemeral) did not. Reload each tab at its history position.
    pub fn restore(&mut self) {
        for idx in 0..self.tabs.len() {
            let t = &mut self.tabs[idx];
            t.doc = 0;
            t.doc_seq = 0;
            t.doc_parsed = false;
            t.loading = None;
            let url = t.history.get(t.hist_pos as usize).cloned().unwrap_or_else(|| t.url.clone());
            let pos = t.hist_pos as usize;
            self.navigate(idx, &url, NavMode::Traverse(pos));
        }
        if self.tab_index(self.session.active_tab).is_none() {
            self.session.active_tab = self.tabs[0].id;
        }
    }

    // ── Navigation ───────────────────────────────────────────────────────

    pub fn navigate(&mut self, idx: usize, raw: &str, mode: NavMode) {
        let url = fixup_url(raw);
        if let Some(r) = self.tabs[idx].loading.take() {
            self.db.delete_request(r);
        }
        if url.starts_with("about:") {
            let html = match url.as_str() {
                "about:blank" => "<html><body></body></html>".to_string(),
                _ => pages::newtab(),
            };
            self.commit(idx, &url, html, mode);
            return;
        }
        if url.starts_with("data:") {
            let bytes = providers::decode_data_url(&url).unwrap_or_default();
            let html = crate::encoding::decode_body(&bytes);
            self.commit(idx, &url, html, mode);
            return;
        }
        if parse_url(&url).is_none() {
            let html = pages::error(&url, "unsupported address");
            self.commit(idx, &url, html, mode);
            return;
        }
        let req_id = self.alloc_req();
        let (nav_mode, nav_index) = mode.encode();
        let tab = &mut self.tabs[idx];
        self.db.insert_request(Request {
            req_id,
            kind: REQ_MAIN,
            url: url.clone(),
            tab: tab.id,
            nav_mode,
            nav_index,
            sent: false,
        });
        tab.loading = Some(req_id);
        if tab.doc == 0 {
            tab.url = url;
        }
    }

    fn alloc_req(&mut self) -> u64 {
        let id = self.session.next_req;
        self.session.next_req += 1;
        id
    }

    fn commit(&mut self, idx: usize, url: &str, html: String, mode: NavMode) {
        let doc_id = self.session.next_doc;
        self.session.next_doc += 1;
        let tab = &mut self.tabs[idx];
        if tab.doc != 0 {
            self.db.drop_document(tab.doc);
            self.caches.docs.remove(&tab.doc);
        }
        self.db.insert_document(Document { id: doc_id, tab: tab.id, url: url.to_string(), html, t0: crate::now_ms() });
        tab.doc = doc_id;
        tab.doc_seq = 0;
        tab.doc_parsed = false;
        tab.url = url.to_string();
        tab.title = if url.starts_with("about:newtab") { "New Tab".into() } else { url.to_string() };
        tab.favicon = None;
        tab.loading = None;
        let pos = tab.hist_pos as usize;
        match mode {
            NavMode::Push => {
                if !tab.history.is_empty() {
                    tab.history.truncate(pos + 1);
                }
                tab.history.push(url.to_string());
                tab.hist_pos = tab.history.len() as u32 - 1;
            }
            NavMode::Replace => {
                if tab.history.is_empty() {
                    tab.history.push(url.to_string());
                    tab.hist_pos = 0;
                } else {
                    tab.history[pos] = url.to_string();
                }
            }
            NavMode::Traverse(i) => {
                if i < tab.history.len() {
                    tab.hist_pos = i as u32;
                    tab.history[i] = url.to_string();
                } else {
                    tab.history.push(url.to_string());
                    tab.hist_pos = tab.history.len() as u32 - 1;
                }
            }
        }
        if idx == self.active() && !url.starts_with("about:newtab") {
            self.blur_url();
        }
    }

    pub fn back(&mut self) {
        let i = self.active();
        let t = &self.tabs[i];
        if t.hist_pos > 0 {
            let target = t.hist_pos as usize - 1;
            let url = t.history[target].clone();
            self.navigate(i, &url, NavMode::Traverse(target));
        }
    }

    pub fn forward(&mut self) {
        let i = self.active();
        let t = &self.tabs[i];
        if (t.hist_pos as usize) + 1 < t.history.len() {
            let target = t.hist_pos as usize + 1;
            let url = t.history[target].clone();
            self.navigate(i, &url, NavMode::Traverse(target));
        }
    }

    pub fn reload(&mut self) {
        let i = self.active();
        let url = self.tabs[i].url.clone();
        if !url.is_empty() {
            let pos = self.tabs[i].hist_pos as usize;
            self.navigate(i, &url, NavMode::Traverse(pos));
        }
    }

    pub fn stop(&mut self) {
        let i = self.active();
        if let Some(r) = self.tabs[i].loading.take() {
            self.db.delete_request(r);
        }
    }

    pub fn go_home(&mut self) {
        let home = self.session.home.clone();
        let i = self.active();
        self.navigate(i, &home, NavMode::Push);
    }

    // ── Network ──────────────────────────────────────────────────────────

    /// A response for one of our `Request`s (already removed from the table).
    pub fn on_response(&mut self, req: Request, body: Vec<u8>, error: &str, final_url: Option<String>) {
        match req.kind {
            REQ_SUB => self.store_resource(&req.url, body),
            REQ_FAVICON => {
                let icon = sniff_mime(&body)
                    .filter(|_| body.len() < 256 * 1024)
                    .map(|mime| format!("data:{mime};base64,{}", providers::base64_encode(&body)));
                if let (Some(idx), Some(icon)) = (self.tab_index(req.tab), icon) {
                    self.tabs[idx].favicon = Some(icon);
                }
            }
            _ => {
                let Some(idx) = self.tab_index(req.tab) else { return };
                if self.tabs[idx].loading != Some(req.req_id) {
                    return;
                }
                let url = req.url;
                let mut final_url = final_url.unwrap_or_else(|| url.clone());
                // Keep the fragment of the requested URL across redirects.
                if let (Ok(r), Ok(mut fin)) = (url::Url::parse(&url), url::Url::parse(&final_url)) {
                    if fin.fragment().is_none() && r.fragment().is_some() {
                        fin.set_fragment(r.fragment());
                        final_url = fin.to_string();
                    }
                }
                let html = if !error.is_empty() && body.is_empty() {
                    pages::error(&final_url, error)
                } else if sniff_mime(&body).is_some_and(|m| m != "image/svg+xml") {
                    // Show the image itself, served from the resource cache.
                    self.store_resource(&final_url, body);
                    pages::image_page(&final_url)
                } else {
                    let text = crate::encoding::decode_body(&body);
                    if looks_like_html(&body) || body.is_empty() { text } else { pages::text_page(&text) }
                };
                self.commit(idx, &final_url, html, NavMode::decode(req.nav_mode, req.nav_index));
            }
        }
    }

    fn store_resource(&mut self, url: &str, body: Vec<u8>) {
        if self.db.has_resource(url) {
            return;
        }
        let size = body.len() as u64;
        self.session.res_clock += 1;
        self.db.insert_resource(url, body, self.session.res_clock);
        self.session.res_bytes += size;
        self.session.res_rev += 1;
        while self.session.res_bytes > RESOURCE_BUDGET {
            match self.db.evict_oldest_resource() {
                Some(sz) => self.session.res_bytes = self.session.res_bytes.saturating_sub(sz),
                None => break,
            }
        }
    }

    /// Hand sub-resource fetches queued by documents to the resource cache,
    /// or request them.
    /// Returns whether there was anything to route.
    fn pump_fetches(&mut self) -> bool {
        let fetches: Vec<PendingFetch> = std::mem::take(&mut *providers::NET_QUEUE.lock().unwrap());
        let any = !fetches.is_empty();
        for f in fetches {
            let url = f.url.to_string();
            let Some(cache) = self.caches.docs.values_mut().find(|c| c.doc.id() == f.doc_id) else {
                f.handler.bytes(url, Bytes::new());
                continue;
            };
            if let Some(body) = self.db.resource(&url) {
                f.handler.bytes(url, Bytes::from(body));
                cache.dirty = true;
                continue;
            }
            cache.waiting.entry(url.clone()).or_default().push(f.handler);
            if !self.db.request_pending_for(&url) {
                let req_id = self.session.next_req;
                self.session.next_req += 1;
                self.db.insert_request(Request {
                    req_id,
                    kind: REQ_SUB,
                    url,
                    tab: 0,
                    nav_mode: 0,
                    nav_index: 0,
                    sent: false,
                });
            }
        }
        any
    }

    /// Deliver newly cached resources to the active document's waiting fetches.
    fn deliver_resources(&mut self) {
        let rev = self.session.res_rev;
        let db = self.db;
        let Some(cache) = self.active_cache() else { return };
        if cache.res_rev == rev || cache.waiting.is_empty() {
            return;
        }
        cache.res_rev = rev;
        let urls: Vec<String> = cache.waiting.keys().cloned().collect();
        for url in urls {
            if let Some(body) = db.resource(&url) {
                let body = Bytes::from(body);
                for h in cache.waiting.remove(&url).unwrap_or_default() {
                    h.bytes(url.clone(), body.clone());
                }
                cache.dirty = true;
            }
        }
    }

    /// Navigations requested by documents (link clicks, form submits).
    fn pump_navigations(&mut self) {
        let navs = std::mem::take(&mut *providers::NAV_QUEUE.lock().unwrap());
        for n in navs {
            let doc = self
                .caches
                .docs
                .iter()
                .find(|(_, c)| c.doc.id() == n.source_document)
                .map(|(id, _)| *id);
            let idx = doc
                .and_then(|d| self.tabs.iter().position(|t| t.doc == d))
                .unwrap_or_else(|| self.active());
            self.navigate(idx, n.url.as_str(), NavMode::Push);
        }
    }

    // ── Document caches ──────────────────────────────────────────────────

    /// Make sure the active tab's document is built and up to date with its
    /// `DocEvent` log.
    pub fn ensure_active_doc(&mut self, now: f64) {
        // Drop caches of documents no tab shows anymore.
        let live: Vec<u64> = self.tabs.iter().map(|t| t.doc).collect();
        self.caches.docs.retain(|id, _| live.contains(id));

        let idx = self.active();
        let Some(tab) = self.tabs.get(idx) else { return };
        let (doc_id, doc_seq) = (tab.doc, tab.doc_seq);
        if doc_id == 0 {
            return;
        }
        let mut built = false;
        let mut boot = None;
        if !self.caches.docs.contains_key(&doc_id) {
            let Some(row) = self.db.document(doc_id) else { return };
            boot = Some((row.t0, row.url.clone()));
            let mut doc = HtmlDocument::from_html(&row.html, self.doc_config(&row.url));
            if let Some(f) = url::Url::parse(&row.url).ok().and_then(|u| u.fragment().map(str::to_string)) {
                doc.resolve(now);
                doc.scroll_to_fragment(&f);
            }
            self.caches.docs.insert(doc_id, DocCache::new(doc));
            built = true;
        }
        // Sub-resources from the resource cache; a few rounds, since
        // stylesheets pull in fonts/images of their own. Only resolves when
        // there is something new (a fresh document or fetches to route):
        // `resolve` walks the whole tree, so idle frames must not call it.
        let mut fresh = built;
        for _ in 0..4 {
            let routed = self.pump_fetches();
            if !fresh && !routed {
                break;
            }
            fresh = false;
            let Some(c) = self.caches.docs.get_mut(&doc_id) else { return };
            c.doc.resolve(now);
            if providers::NET_QUEUE.lock().unwrap().is_empty() {
                break;
            }
        }
        // Viewport is an idempotent setter from `Ui.viewport`.
        let (w, h) = self.content_size();
        let c = self.caches.docs.get_mut(&doc_id).unwrap();
        if c.doc.viewport().window_size != (w, h) {
            c.doc.set_viewport(Viewport::new(w, h, 1.0, ColorScheme::Light));
            c.dirty = true;
        }
        // Run the page's scripts once its sub-resources are in. Deterministic
        // from the `Document` row, so a rebuild re-runs them before replaying
        // the log; side effects only on the first (live) run.
        if let Some((t0, url)) = boot {
            if js::has_scripts(&c.doc) {
                let first = !self.tabs[idx].doc_parsed;
                let mut realm = Js::new(doc_id, t0, self.db.web_storage(&origin_of(&url)));
                realm.host().live = first;
                c.doc.resolve(now);
                let t = crate::now_ms();
                realm.boot(&mut c.doc);
                crate::log(&format!("js: boot doc {doc_id} in {:.0} ms", crate::now_ms() - t));
                realm.host().live = false;
                if !first {
                    realm.host().storage_writes.clear();
                    providers::NAV_QUEUE.lock().unwrap().clear();
                }
                c.js = Some(realm);
                c.dirty = true;
            }
        }
        // Catch up on events applied by other instances (replay: no side effects).
        if c.ev_seq < doc_seq {
            let after = c.ev_seq;
            let events: Vec<DocEvent> = self.db.doc_events(doc_id).into_iter().filter(|e| e.seq > after).collect();
            let db = self.db;
            let c = self.caches.docs.get_mut(&doc_id).unwrap();
            for e in &events {
                c.doc.resolve(now);
                apply_doc_event(c, e, db);
                c.ev_seq = e.seq;
            }
            c.logged += events.len();
            c.ev_seq = c.ev_seq.max(doc_seq);
            c.dirty = true;
            if let Some(realm) = c.js.as_mut() {
                realm.host().storage_writes.clear();
            }
            providers::NAV_QUEUE.lock().unwrap().clear();
            self.pump_fetches();
        }
        // First build of a fresh document: derive title + favicon into the tab.
        if built && !self.tabs[idx].doc_parsed {
            self.extract_metadata(idx);
        }
    }

    fn extract_metadata(&mut self, idx: usize) {
        let doc_id = self.tabs[idx].doc;
        let Some(c) = self.caches.docs.get(&doc_id) else { return };
        let doc: &BaseDocument = &c.doc;
        let title = doc
            .find_title_node()
            .map(|n| n.text_content().split_whitespace().collect::<Vec<_>>().join(" "))
            .unwrap_or_default();
        let url = self.tabs[idx].url.clone();
        let favicon = url::Url::parse(&url).ok().filter(|b| matches!(b.scheme(), "http" | "https")).and_then(|base| {
            let href = ["link[rel~=\"icon\"]", "link[rel=\"shortcut icon\"]"]
                .iter()
                .filter_map(|sel| doc.query_selector(sel).ok().flatten())
                .filter_map(|id| doc.get_node(id)?.attr(blitz_dom::local_name!("href")).map(str::to_string))
                .next();
            match href {
                Some(h) => base.join(&h).ok(),
                None => base.join("/favicon.ico").ok(),
            }
        });
        let tab = &mut self.tabs[idx];
        tab.doc_parsed = true;
        if !title.is_empty() {
            tab.title = title;
        }
        if let Some(fav) = favicon.filter(|f| f.scheme() != "data") {
            let tab_id = tab.id;
            match self.db.resource(fav.as_str()) {
                Some(body) => {
                    if let Some(mime) = sniff_mime(&body) {
                        self.tabs[idx].favicon =
                            Some(format!("data:{mime};base64,{}", providers::base64_encode(&body)));
                    }
                }
                None => {
                    let req_id = self.alloc_req();
                    self.db.insert_request(Request {
                        req_id,
                        kind: REQ_FAVICON,
                        url: fav.to_string(),
                        tab: tab_id,
                        nav_mode: 0,
                        nav_index: 0,
                        sent: false,
                    });
                }
            }
        }
    }

    /// Build the chrome document if needed and sync it with the tables
    /// (idempotent setters: only what differs from the applied view changes).
    pub fn sync_chrome(&mut self) {
        let w = self.ui.viewport.0.max(1);
        let c = self.caches.chrome.get_or_insert_with(|| ChromeCache::new(chrome::build(w)));
        if c.doc.viewport().window_size != (w, CHROME_H) {
            c.doc.set_viewport(Viewport::new(w, CHROME_H, 1.0, ColorScheme::Light));
            c.dirty = true;
        }
        let active = self.tabs.iter().position(|t| t.id == self.session.active_tab).unwrap_or(0);
        let t = &self.tabs[active];
        let view = ChromeView {
            tabs: self
                .tabs
                .iter()
                .map(|t| (t.id, t.title.clone(), t.loading.is_some(), t.id == self.session.active_tab, t.favicon.clone()))
                .collect(),
            nav: (t.hist_pos > 0, (t.hist_pos as usize) + 1 < t.history.len(), t.loading.is_some()),
            url: t.url.clone(),
            editing: self.ui.url_editing,
            url_text: self.ui.url_text.clone(),
        };
        let old = c.applied.take();
        let doc: &mut BaseDocument = &mut c.doc;
        let mut changed = false;
        if old.as_ref().map(|o| &o.tabs) != Some(&view.tabs) {
            let views: Vec<TabView> = view
                .tabs
                .iter()
                .map(|(id, title, loading, selected, fav)| TabView {
                    id: *id,
                    title,
                    loading: *loading,
                    selected: *selected,
                    favicon: fav.as_deref(),
                })
                .collect();
            chrome::set_tabs(doc, &views);
            changed = true;
        }
        if old.as_ref().map(|o| o.nav) != Some(view.nav) {
            chrome::set_nav_state(doc, view.nav.0, view.nav.1, view.nav.2);
            changed = true;
        }
        let edit_changed = old.as_ref().map(|o| (o.editing, &o.url_text)) != Some((view.editing, &view.url_text));
        if edit_changed {
            chrome::set_url_editing(doc, view.editing, &view.url_text);
            changed = true;
        }
        if !view.editing && (edit_changed || old.as_ref().map(|o| &o.url) != Some(&view.url)) {
            chrome::set_url_display(doc, &view.url);
            changed = true;
        }
        c.dirty |= changed;
        c.applied = Some(view);
    }

    // ── URL field ────────────────────────────────────────────────────────

    pub fn focus_url(&mut self) {
        let url = self.tabs[self.active()].url.clone();
        self.ui.url_text = if url.starts_with("about:newtab") { String::new() } else { url };
        self.ui.url_editing = true;
        self.ui.focus_chrome = true;
        // Re-apply (select-all) even if the text did not change.
        if let Some(a) = self.caches.chrome.as_mut().and_then(|c| c.applied.as_mut()) {
            a.editing = false;
        }
    }

    pub fn blur_url(&mut self) {
        self.ui.url_editing = false;
        self.ui.url_text.clear();
        self.ui.focus_chrome = false;
    }

    fn apply(&mut self, action: ChromeAction) {
        match action {
            ChromeAction::Back => self.back(),
            ChromeAction::Forward => self.forward(),
            ChromeAction::Reload => {
                if self.tabs[self.active()].loading.is_some() { self.stop() } else { self.reload() }
            }
            ChromeAction::Home => self.go_home(),
            ChromeAction::NewTab => self.new_tab("about:newtab"),
            ChromeAction::SelectTab(id) => {
                if let Some(i) = self.tab_index(id) {
                    self.select_tab(i)
                }
            }
            ChromeAction::CloseTab(id) => {
                if let Some(i) = self.tab_index(id) {
                    self.close_tab(i)
                }
            }
            ChromeAction::Go(text) => {
                self.blur_url();
                let i = self.active();
                self.navigate(i, &text, NavMode::Push);
            }
            ChromeAction::FocusUrl => {
                if !self.ui.url_editing {
                    self.focus_url()
                }
            }
            ChromeAction::RevertUrl => self.blur_url(),
            ChromeAction::Menu | ChromeAction::ListTabs => {}
        }
    }

    // ── Input ────────────────────────────────────────────────────────────

    fn region_at(&self, y: f32) -> u8 {
        if y < CHROME_H as f32 { REGION_CHROME } else { REGION_CONTENT }
    }

    fn send_chrome(&mut self, ev: UiEvent) -> Vec<ChromeAction> {
        let mut actions = Vec::new();
        let hover_only = matches!(ev, UiEvent::PointerMove(_) | UiEvent::PointerLeave(_));
        let Some(c) = self.caches.chrome.as_mut() else { return actions };
        {
            let doc: &mut dyn blitz_dom::Document = &mut c.doc;
            let mut driver = EventDriver::new(doc, ChromeHandler { actions: &mut actions });
            driver.handle_ui_event(ev);
        }
        // Pointer motion repaints only through Blitz's redraw request (hover
        // change); everything else may edit the chrome without asking.
        c.dirty |= !hover_only;
        // The URL field's text is table state: read edits back.
        if self.ui.url_editing {
            let text = chrome::url_text(&c.doc);
            if let Some(a) = c.applied.as_mut() {
                a.url_text = text.clone();
            }
            self.ui.url_text = text;
        }
        actions
    }

    /// Apply an input event to the active document *live*: log it, then run
    /// it (side effects such as link navigations are picked up afterwards).
    fn send_content(&mut self, mut ev: DocEvent) {
        let idx = self.active();
        let doc_id = self.tabs[idx].doc;
        if !self.caches.docs.contains_key(&doc_id) {
            return;
        }
        ev.doc = doc_id;
        let seq = self.db.push_doc_event(ev.clone());
        ev.seq = seq;
        let db = self.db;
        let c = self.caches.docs.get_mut(&doc_id).unwrap();
        if let Some(realm) = c.js.as_mut() {
            realm.host().live = true;
        }
        apply_doc_event(c, &ev, db);
        if let Some(realm) = c.js.as_mut() {
            realm.host().live = false;
        }
        c.ev_seq = seq;
        c.dirty = true;
        c.logged += 1;
        self.tabs[idx].doc_seq = seq;
        if c.logged > MAX_DOC_EVENTS {
            self.compact_log(idx);
        }
    }

    /// Replace the active document's event log with one `EV_SCROLL_TO`
    /// snapshot. Exact replay of an unbounded log (a `requestAnimationFrame`
    /// loop logs 60 ticks/s) is not worth its storage and rebuild time; a
    /// rebuild then shows the freshly loaded page at the same scroll offset.
    /// The live cache is untouched: it already holds the state.
    fn compact_log(&mut self, idx: usize) {
        let doc_id = self.tabs[idx].doc;
        let Some(c) = self.caches.docs.get_mut(&doc_id) else { return };
        let p = c.doc.viewport_scroll();
        self.db.clear_doc_events(doc_id);
        let mut ev = self.content_event(EV_SCROLL_TO);
        (ev.doc, ev.dx, ev.dy) = (doc_id, p.x as f32, p.y as f32);
        let seq = self.db.push_doc_event(ev);
        let c = self.caches.docs.get_mut(&doc_id).unwrap();
        (c.ev_seq, c.logged) = (seq, 1);
        self.tabs[idx].doc_seq = seq;
    }

    fn content_event(&self, kind: u8) -> DocEvent {
        let (x, y) = self.ui.mouse;
        DocEvent {
            seq: 0,
            doc: 0,
            kind,
            x,
            y: y - CHROME_H as f32,
            button: 0,
            buttons: self.ui.buttons,
            mods: self.ui.mods,
            code: 0,
            repeat: false,
            dx: 0.0,
            dy: 0.0,
            key: String::new(),
            text: String::new(),
        }
    }

    /// Hover is not logged: it is re-derived from the pointer position.
    /// Nothing is marked dirty here: Blitz requests a redraw (the provider's
    /// flag, read in `update`) only when the hovered element actually changes,
    /// so moving within one element costs no repaint.
    /// Hover-only pointer motion (not logged: scripts don't observe it).
    fn hover_content(&mut self, ev: UiEvent) {
        if let Some(c) = self.active_cache() {
            let ev = match ev {
                UiEvent::PointerMove(p) => UiEvent::PointerMove(scrolled(p, &c.doc)),
                other => other,
            };
            c.doc.handle_ui_event(ev);
        }
    }

    fn js_wants_motion(&self) -> bool {
        let doc = self.tabs.get(self.active()).map(|t| t.doc).unwrap_or(0);
        self.caches.docs.get(&doc).and_then(|c| c.js.as_ref()).is_some_and(|j| j.wants_motion())
    }

    pub fn mouse_move(&mut self, x: f32, y: f32) {
        if (x, y) == self.ui.mouse {
            return;
        }
        self.ui.mouse = (x, y);
        let (buttons, mods) = (self.buttons(), self.mods());
        let region = self.region_at(y);
        if region != self.ui.region {
            // Leave the previous document so its hover state clears.
            let leave = pointer(-1.0, -1.0, MouseEventButton::Main, buttons, mods);
            match self.ui.region {
                REGION_CHROME => {
                    self.send_chrome(UiEvent::PointerLeave(leave));
                }
                REGION_CONTENT if self.js_wants_motion() => {
                    let mut ev = self.content_event(EV_MOVE);
                    (ev.x, ev.y) = (-1.0, -1.0);
                    self.send_content(ev);
                }
                REGION_CONTENT => self.hover_content(UiEvent::PointerLeave(leave)),
                _ => {}
            }
            self.ui.region = region;
        }
        if region == REGION_CHROME {
            let acts = self.send_chrome(UiEvent::PointerMove(pointer(x, y, MouseEventButton::Main, buttons, mods)));
            for a in acts {
                self.apply(a);
            }
        } else if self.js_wants_motion() {
            // Scripts observe motion: it becomes part of the document's log.
            let ev = self.content_event(EV_MOVE);
            self.send_content(ev);
        } else {
            let cy = y - CHROME_H as f32;
            self.hover_content(UiEvent::PointerMove(pointer(x, cy, MouseEventButton::Main, buttons, mods)));
        }
    }

    /// Wheel at the pointer, in pixels, Blitz/winit sign: positive = scroll
    /// up/left (content moves down/right).
    pub fn wheel(&mut self, dx: f32, dy: f32) {
        if self.region_at(self.ui.mouse.1) != REGION_CONTENT {
            return;
        }
        let mut ev = self.content_event(EV_WHEEL);
        ev.dx = dx;
        ev.dy = dy;
        self.send_content(ev);
    }

    pub fn button(&mut self, id: u8, pressed: bool) {
        let button = button_of(id);
        let flag: MouseEventButtons = button.into();
        let mut buttons = self.buttons();
        if pressed { buttons |= flag } else { buttons &= !flag }
        self.ui.buttons = buttons.bits();
        let region = self.region_at(self.ui.mouse.1);
        if pressed && button == MouseEventButton::Main {
            match region {
                REGION_CONTENT => {
                    if self.ui.url_editing {
                        self.blur_url();
                    }
                    self.ui.focus_chrome = false;
                }
                REGION_CHROME => self.ui.focus_chrome = true,
                _ => {}
            }
        }
        if region == REGION_CONTENT {
            let mut ev = self.content_event(if pressed { EV_DOWN } else { EV_UP });
            ev.button = id;
            self.send_content(ev);
            return;
        }
        let (x, y) = self.ui.mouse;
        let p = pointer(x, y, button, buttons, self.mods());
        let acts = self.send_chrome(if pressed { UiEvent::PointerDown(p) } else { UiEvent::PointerUp(p) });
        let mut focused_url = false;
        for a in acts {
            focused_url |= a == ChromeAction::FocusUrl;
            self.apply(a);
        }
        // A press in the chrome that didn't land on the URL field ends editing.
        if pressed && self.ui.url_editing && !focused_url && !self.over_url_field() {
            self.blur_url();
            self.ui.focus_chrome = true;
        }
    }

    fn over_url_field(&self) -> bool {
        let Some(c) = self.caches.chrome.as_ref() else { return false };
        let doc: &BaseDocument = &c.doc;
        doc.get_hover_node_id()
            .map(|h| {
                doc.node_chain(h).iter().any(|&n| {
                    doc.get_node(n)
                        .and_then(|n| n.attr(blitz_dom::local_name!("id")))
                        .is_some_and(|i| i == "urlwrap")
                })
            })
            .unwrap_or(false)
    }

    /// Browser-level keyboard shortcuts. Returns true if consumed.
    fn shortcut(&mut self, name: &str) -> bool {
        let m = self.mods();
        let n = self.tabs.len();
        let active = self.active();
        match (m.ctrl, m.alt, m.shift, name) {
            (true, false, _, "KeyL") | (false, true, false, "KeyD") | (false, false, false, "F6") => self.focus_url(),
            (true, false, false, "KeyT") => self.new_tab("about:newtab"),
            (true, false, false, "KeyW") | (true, false, false, "F4") => self.close_tab(active),
            (true, false, false, "KeyR") | (false, false, false, "F5") => self.reload(),
            (false, true, false, "ArrowLeft") | (false, false, false, "BrowserBack") => self.back(),
            (false, true, false, "ArrowRight") | (false, false, false, "BrowserForward") => self.forward(),
            (false, true, false, "Home") => self.go_home(),
            (true, false, false, "Tab") | (true, false, false, "PageDown") => self.select_tab((active + 1) % n),
            (true, false, true, "Tab") | (true, false, false, "PageUp") => self.select_tab((active + n - 1) % n),
            (true, false, false, d) if d.starts_with("Digit") && d != "Digit0" => {
                let k: usize = d[5..].parse().unwrap_or(1);
                let idx = if k == 9 { n - 1 } else { k - 1 };
                self.select_tab(idx.min(n - 1));
            }
            _ => return false,
        }
        true
    }

    /// A key transition: physical `code`, plus the logical `key` and typed
    /// `text` the input module's keyboard layout produced ("" = unknown).
    pub fn key(&mut self, code: u32, pressed: bool, repeat: bool, key: &str, text: &str) {
        let name = keys::code_name(code).unwrap_or("");
        if name.is_empty() && key.is_empty() {
            return;
        }
        let mut mods = self.mods();
        mods.update(name, pressed);
        self.ui.mods = mods.bits();
        if pressed && !repeat && self.shortcut(&keys::shortcut_name(name, key)) {
            return;
        }
        let active = self.active();
        if pressed && name == "Escape" && self.tabs[active].loading.is_some() && !self.ui.url_editing {
            self.stop();
            return;
        }
        let Some(mut ev) = keys::key_event(name, key, text, pressed, mods) else { return };
        ev.is_auto_repeating = repeat;
        if self.ui.focus_chrome {
            let ui = if pressed { UiEvent::KeyDown(ev) } else { UiEvent::KeyUp(ev) };
            let acts = self.send_chrome(ui);
            for a in acts {
                self.apply(a);
            }
            return;
        }
        // Keyboard scrolling unless an editable control has focus. Clicking
        // the page focuses `<html>`, which must not swallow PageDown & co.
        let editing = self
            .active_cache()
            .and_then(|c| c.doc.get_focussed_node_id().and_then(|id| c.doc.get_node(id)))
            .and_then(|n| n.element_data())
            .map(|e| {
                e.text_input_data().is_some()
                    || &*e.name.local == "select"
                    || e.attr(blitz_dom::local_name!("contenteditable")).is_some_and(|v| v != "false")
            })
            .unwrap_or(false);
        if pressed && !editing && !mods.ctrl && !mods.alt {
            let page = self.content_size().1 as f32 * 0.85;
            let delta = match &ev.key {
                Key::Named(NamedKey::ArrowDown) => Some((0.0, 48.0)),
                Key::Named(NamedKey::ArrowUp) => Some((0.0, -48.0)),
                Key::Named(NamedKey::ArrowRight) => Some((48.0, 0.0)),
                Key::Named(NamedKey::ArrowLeft) => Some((-48.0, 0.0)),
                Key::Named(NamedKey::PageDown) => Some((0.0, page)),
                Key::Named(NamedKey::PageUp) => Some((0.0, -page)),
                Key::Named(NamedKey::Home) => Some((0.0, -1e9)),
                Key::Named(NamedKey::End) => Some((0.0, 1e9)),
                Key::Character(c) if c == " " => Some((0.0, if mods.shift { -page } else { page })),
                _ => None,
            };
            if let Some((dx, dy)) = delta {
                let mut e = self.content_event(EV_SCROLL);
                e.dx = dx;
                e.dy = dy;
                self.send_content(e);
                return;
            }
        }
        let mut e = self.content_event(if pressed { EV_KEY_DOWN } else { EV_KEY_UP });
        e.code = code;
        e.repeat = repeat;
        e.key = key.into();
        e.text = text.into();
        self.send_content(e);
    }

    /// One headless test command (`browser_script` lines, after the frame):
    /// - `click X Y` / `move X Y`: pointer, window coordinates;
    /// - `wheel DX DY`: wheel at the pointer, pixels (positive = down/right);
    /// - `key NAME[+NAME…]`: press then release (`Control+KeyL`, `PageDown`);
    /// - `type TEXT`: US-layout key presses for each character (others, e.g.
    ///   `é`, as keys with an unknown code and that logical key / text);
    /// - `keyas [MOD+…]CODE KEY [TEXT]`: press + release of physical `CODE`
    ///   that the layout reports as `KEY` typing `TEXT`, modifiers held
    ///   (AZERTY: `keyas KeyQ a a`, `keyas Control+KeyZ w`);
    /// - `dump`: log the browser's state as one `browser-test` line;
    /// - `js EXPR`: log an expression evaluated in the page (read-only use).
    pub fn script(&mut self, cmd: &str, frame: u64, now: f64) {
        let (verb, rest) = cmd.split_once(char::is_whitespace).unwrap_or((cmd, ""));
        let nums: Vec<f32> = rest.split_whitespace().filter_map(|n| n.parse().ok()).collect();
        let press = |b: &mut Self, name: &str, down: bool| match keys::code_of(name) {
            Some(code) => b.key(code, down, false, "", ""),
            None => crate::log(&format!("browser-test: unknown key {name:?}")),
        };
        match (verb, nums.as_slice()) {
            ("click", [x, y]) => {
                self.mouse_move(*x, *y);
                self.button(0, true);
                self.button(0, false);
            }
            ("move", [x, y]) => self.mouse_move(*x, *y),
            ("wheel", [dx, dy]) => self.wheel(-*dx, -*dy),
            ("key", _) => {
                let combo: Vec<&str> = rest.trim().split('+').collect();
                for k in &combo {
                    press(self, k, true);
                }
                for k in combo.iter().rev() {
                    press(self, k, false);
                }
            }
            ("type", _) => {
                for c in rest.chars() {
                    let Some((name, shift)) = keys::code_for_char(c) else {
                        // Not on a US keyboard: send it as the layout would.
                        let s = c.to_string();
                        self.key(u32::MAX, true, false, &s, &s);
                        self.key(u32::MAX, false, false, &s, "");
                        continue;
                    };
                    if shift {
                        press(self, "ShiftLeft", true);
                    }
                    press(self, name, true);
                    press(self, name, false);
                    if shift {
                        press(self, "ShiftLeft", false);
                    }
                }
            }
            ("keyas", _) => {
                let mut it = rest.split_whitespace();
                let (Some(combo), Some(key)) = (it.next(), it.next()) else {
                    crate::log(&format!("browser-test: bad command {cmd:?}"));
                    return;
                };
                let text = it.next().unwrap_or("");
                let (mods, last) = combo.rsplit_once('+').unwrap_or(("", combo));
                let mods: Vec<&str> = mods.split('+').filter(|m| !m.is_empty()).collect();
                let Some(code) = keys::code_of(last) else {
                    crate::log(&format!("browser-test: unknown key {last:?}"));
                    return;
                };
                for m in &mods {
                    press(self, m, true);
                }
                self.key(code, true, false, key, text);
                self.key(code, false, false, key, "");
                for m in mods.iter().rev() {
                    press(self, m, false);
                }
            }
            ("dump", _) => crate::log(&format!("browser-test frame={frame} {}", self.test_state())),
            ("js", _) => {
                let doc_id = self.tabs.get(self.active()).map_or(0, |t| t.doc);
                let out = match self.caches.docs.get_mut(&doc_id) {
                    Some(DocCache { doc, js: Some(realm), .. }) => {
                        doc.resolve(now);
                        realm.eval(doc, rest)
                    }
                    _ => "<no script realm>".into(),
                };
                crate::log(&format!("browser-test frame={frame} js={out:?}"));
            }
            _ => crate::log(&format!("browser-test: bad command {cmd:?}")),
        }
    }

    /// `key=value` summary of the state tests assert on.
    fn test_state(&self) -> String {
        let idx = self.active();
        let Some(tab) = self.tabs.get(idx) else { return "tabs=0".into() };
        // Long `data:` URLs would drown the log; tests match on prefixes.
        let cut = |s: &str| s.chars().take(60).collect::<String>();
        let mut s = format!(
            "tabs={} active={idx} url={:?} title={:?} url_editing={} url_text={:?} loading={}",
            self.tabs.len(),
            cut(&tab.url),
            cut(&tab.title),
            self.ui.url_editing,
            cut(&self.ui.url_text),
            tab.loading.is_some(),
        );
        if let Some(c) = self.caches.docs.get(&tab.doc) {
            let p = c.doc.viewport_scroll();
            s += &format!(" scroll={},{} events={}", p.x.round(), p.y.round(), c.logged);
            let focus = c.doc.get_focussed_node_id().and_then(|id| c.doc.get_node(id)).and_then(|n| {
                let e = n.element_data()?;
                Some(format!("{}#{}", e.name.local, e.attr(blitz_dom::local_name!("id")).unwrap_or("")))
            });
            s += &format!(" focus={}", focus.unwrap_or_else(|| "-".into()));
        }
        s
    }

    pub fn input(&mut self, ev: &PendingInput) {
        match ev.kind {
            IN_KEY => self.key(ev.code, ev.pressed, ev.repeat, &ev.key, &ev.text),
            _ => self.button(ev.code as u8, ev.pressed),
        }
    }

    // ── Frame ────────────────────────────────────────────────────────────

    /// After input: run what documents asked for, then resolve style/layout.
    pub fn update(&mut self, now: f64) {
        self.pump_navigations();
        let _ = self.pump_fetches();
        // Navigations above may have switched documents.
        self.ensure_active_doc(now);
        self.pump_js(now);
        self.deliver_resources();
        let _ = self.pump_fetches();
        self.sync_chrome();
        let chrome_redraw = providers::take_chrome_redraw();
        let redraw = providers::take_redraw();
        // Restyle/relayout only what changed: `dirty` (we mutated it), a
        // redraw Blitz asked for (hover, loaded image …) or an animation.
        if let Some(c) = self.caches.chrome.as_mut() {
            if c.dirty || chrome_redraw || c.doc.is_animating() {
                c.doc.resolve(now);
                c.dirty = true;
            }
        }
        if let Some(c) = self.active_cache() {
            if c.dirty || redraw || c.doc.is_animating() {
                c.doc.resolve(now);
                c.dirty = true;
            }
        }
    }

    /// Feed the active document's scripts what they wait for — fetch
    /// completions and due timers — as logged events, and store their
    /// `localStorage` writes.
    fn pump_js(&mut self, now: f64) {
        let Some(tab) = self.tabs.get(self.active()) else { return };
        let (doc_id, url) = (tab.doc, tab.url.clone());
        let Some(realm) = self.caches.docs.get_mut(&doc_id).and_then(|c| c.js.as_mut()) else { return };
        let writes = std::mem::take(&mut realm.host().storage_writes);
        let fetches: Vec<(u32, String)> = realm.host().fetches.iter().map(|(k, v)| (*k, v.clone())).collect();
        if !writes.is_empty() {
            let origin = origin_of(&url);
            for (name, value) in writes {
                self.db.set_web_storage(&origin, &name, value);
            }
        }
        for (id, furl) in fetches {
            let fetchable = furl.starts_with("http://") || furl.starts_with("https://");
            if !fetchable || self.db.has_resource(&furl) {
                let mut ev = self.content_event(EV_FETCH);
                ev.code = id;
                self.send_content(ev);
            } else if !self.db.request_pending_for(&furl) {
                let req_id = self.alloc_req();
                self.db.insert_request(Request { req_id, kind: REQ_SUB, url: furl, tab: 0, nav_mode: 0, nav_index: 0, sent: false });
            }
        }
        let Some(c) = self.caches.docs.get_mut(&doc_id) else { return };
        let Some(realm) = c.js.as_mut() else { return };
        let rel = now * 1000.0 - realm.host().t0;
        let Some(due) = realm.next_due(&mut c.doc) else { return };
        if due <= rel {
            let clock = rel.max(realm.host().clock).max(0.0);
            let mut ev = self.content_event(EV_TICK);
            ev.code = clock as u32;
            ev.dx = clock.fract() as f32;
            self.send_content(ev);
        }
    }

    fn alloc_stamp(&mut self) -> u64 {
        let s = self.ui.next_stamp.max(BLANK_STAMP + 1);
        self.ui.next_stamp = s + 1;
        s
    }

    /// Chrome pixels, if the chrome texture doesn't already show them.
    pub fn paint_chrome(&mut self) -> Option<Vec<u8>> {
        let need_stamp = self.caches.chrome.as_ref()?.stamp == 0;
        if need_stamp {
            let s = self.alloc_stamp();
            self.caches.chrome.as_mut()?.stamp = s;
        }
        let w = self.ui.viewport.0.max(1);
        let c = self.caches.chrome.as_mut()?;
        if !c.dirty && self.ui.chrome_stamp == c.stamp {
            return None;
        }
        c.dirty = false;
        self.ui.chrome_stamp = c.stamp;
        Some(paint(&mut c.doc, w, CHROME_H))
    }

    /// Content pixels, if the content texture doesn't already show them.
    /// Content pixels that changed since the texture was last updated:
    /// the whole viewport, or just the damaged rectangle.
    pub fn paint_content(&mut self) -> Option<Painted> {
        let (w, h) = self.content_size();
        let doc_id = self.tabs.get(self.active())?.doc;
        if !self.caches.docs.contains_key(&doc_id) {
            if self.ui.content_stamp == BLANK_STAMP {
                return None;
            }
            self.ui.content_stamp = BLANK_STAMP;
            return Some(Painted { x: 0, y: 0, w, h, pixels: vec![255u8; (w * h * 4) as usize] });
        }
        if self.caches.docs[&doc_id].stamp == 0 {
            let s = self.alloc_stamp();
            self.caches.docs.get_mut(&doc_id)?.stamp = s;
        }
        let c = self.caches.docs.get_mut(&doc_id)?;
        if !c.dirty && self.ui.content_stamp == c.stamp {
            return None;
        }
        c.dirty = false;
        let scene = record(&mut c.doc, w, h);
        // Damage = what differs from the scene the texture already shows;
        // only valid while the texture holds this document's last paint.
        let texture_current =
            self.ui.content_stamp == c.stamp && !self.ui.content_tex.is_empty() && self.ui.content_tex_size == (w, h);
        let damage = match (&c.scene, texture_current) {
            (Some(old), true) => damage(&old.commands, &scene.commands, w, h),
            _ => None,
        };
        self.ui.content_stamp = c.stamp;
        let out = match damage {
            Some((_, _, 0, _)) | Some((_, _, _, 0)) => None,
            Some((x, y, dw, dh)) if (dw * dh) as f64 <= 0.6 * (w * h) as f64 => {
                Some(Painted { x, y, w: dw, h: dh, pixels: raster(scene.clone(), x, y, dw, dh) })
            }
            _ => Some(Painted { x: 0, y: 0, w, h, pixels: raster(scene.clone(), 0, 0, w, h) }),
        };
        c.scene = Some(scene);
        out
    }

    /// Cursor shape for the current pointer position.
    pub fn cursor(&self) -> cursor_icon::CursorIcon {
        let doc: Option<&BaseDocument> = match self.ui.region {
            REGION_CHROME => self.caches.chrome.as_ref().map(|c| &*c.doc),
            _ => self.tabs.get(self.active()).and_then(|t| self.caches.docs.get(&t.doc)).map(|c| &*c.doc),
        };
        doc.and_then(|d| d.get_cursor()).unwrap_or(cursor_icon::CursorIcon::Default)
    }

    /// URL of the link under the pointer (for the status overlay).
    pub fn hovered_link(&self) -> Option<String> {
        if self.ui.region != REGION_CONTENT {
            return None;
        }
        let doc: &BaseDocument = &self.caches.docs.get(&self.tabs.get(self.active())?.doc)?.doc;
        let mut node = doc.get_hover_node_id();
        while let Some(id) = node {
            let n = doc.get_node(id)?;
            if n.data.is_element_with_tag_name(&blitz_dom::local_name!("a")) {
                if let Some(h) = n.attr(blitz_dom::local_name!("href")) {
                    return doc.url().join(h).ok().map(|u| u.to_string());
                }
            }
            node = n.parent;
        }
        None
    }
}

/// Run one logged event on a document. Used for live input and for replay
/// alike, so both produce the same document state.
fn apply_doc_event(c: &mut DocCache, e: &DocEvent, db: &dyn Store) {
    let DocCache { doc, js, .. } = c;
    let mods = Mods::from_bits(e.mods);
    let buttons = MouseEventButtons::from_bits_truncate(e.buttons);
    let button = button_of(e.button);
    let scroll = doc.viewport_scroll();
    let at = |b| {
        let mut p = pointer(e.x, e.y, b, buttons, mods);
        p.coords.page_x += scroll.x as f32;
        p.coords.page_y += scroll.y as f32;
        p
    };
    match e.kind {
        EV_DOWN | EV_UP => {
            send_ui(doc, js, UiEvent::PointerMove(at(MouseEventButton::Main)));
            send_ui(doc, js, if e.kind == EV_DOWN { UiEvent::PointerDown(at(button)) } else { UiEvent::PointerUp(at(button)) });
        }
        EV_MOVE => send_ui(doc, js, UiEvent::PointerMove(at(MouseEventButton::Main))),
        EV_WHEEL => {
            send_ui(doc, js, UiEvent::PointerMove(at(MouseEventButton::Main)));
            send_ui(
                doc,
                js,
                UiEvent::Wheel(BlitzWheelEvent {
                    delta: BlitzWheelDelta::Pixels(e.dx as f64, e.dy as f64),
                    coords: PointerCoords {
                        page_x: e.x + scroll.x as f32,
                        page_y: e.y + scroll.y as f32,
                        screen_x: e.x,
                        screen_y: e.y,
                        client_x: e.x,
                        client_y: e.y,
                    },
                    buttons,
                    mods: mods.to_modifiers(),
                    element: Point::default(),
                }),
            );
        }
        // `scroll_viewport_by` takes wheel-sign deltas (positive = up).
        EV_SCROLL => doc.scroll_viewport_by(-e.dx as f64, -e.dy as f64),
        EV_SCROLL_TO => doc.set_viewport_scroll(blitz_dom::Point { x: e.dx as f64, y: e.dy as f64 }),
        EV_KEY_DOWN | EV_KEY_UP => {
            let name = keys::code_name(e.code).unwrap_or("");
            if let Some(mut k) = keys::key_event(name, &e.key, &e.text, e.kind == EV_KEY_DOWN, mods) {
                k.is_auto_repeating = e.repeat;
                send_ui(doc, js, if e.kind == EV_KEY_DOWN { UiEvent::KeyDown(k) } else { UiEvent::KeyUp(k) });
            }
        }
        EV_TICK => {
            if let Some(js) = js {
                js.tick(doc, e.code as f64 + e.dx as f64);
            }
        }
        EV_FETCH => {
            if let Some(js) = js {
                let url = js.host().fetches.get(&e.code).cloned().unwrap_or_default();
                let body = if url.starts_with("data:") { providers::decode_data_url(&url) } else { db.resource(&url) };
                js.fetch_done(doc, e.code, body.as_deref());
            }
        }
        _ => {}
    }
}

/// Deliver a UI event, through the page's script listeners when it has any.
fn send_ui(doc: &mut HtmlDocument, js: &mut Option<Js>, ev: UiEvent) {
    match js {
        None => doc.handle_ui_event(ev),
        Some(js) => EventDriver::new(doc, JsHandler { js }).handle_ui_event(ev),
    }
}

/// Runs script listeners for each DOM event Blitz dispatches; their
/// `preventDefault()` cancels Blitz's default action (link navigation,
/// focus, text editing …).
struct JsHandler<'a> {
    js: &'a mut Js,
}

impl EventHandler for JsHandler<'_> {
    fn handle_event(&mut self, _chain: &[blitz_dom::NodeId], event: &mut DomEvent, doc: &mut dyn blitz_dom::Document, state: &mut EventState) {
        let name = event.name();
        let mut doc = doc.inner_mut();
        // Full ancestor path (Blitz passes only the target for non-bubbling
        // events; script capture listeners still see them).
        let mut path = Vec::new();
        let mut n = Some(event.target);
        while let Some(id) = n {
            path.push(id);
            n = doc.get_node(id).and_then(|n| n.parent);
        }
        if !self.js.wants(&doc, name, &path) {
            return;
        }
        let init = event_init(&event.data);
        let (prevented, stopped) = self.js.dispatch(&mut doc, name, &path, event.bubbles, event.cancelable, &init);
        if prevented {
            state.prevent_default();
        }
        if stopped {
            state.stop_propagation();
        }
        state.request_redraw();
    }
}

/// The JS event init dictionary (as JSON) for a Blitz DOM event.
fn event_init(data: &DomEventData) -> String {
    use keyboard_types::Modifiers as M;
    use DomEventData as D;
    let mods = |m: M| {
        format!(
            "\"ctrlKey\":{},\"shiftKey\":{},\"altKey\":{},\"metaKey\":{}",
            m.contains(M::CONTROL),
            m.contains(M::SHIFT),
            m.contains(M::ALT),
            m.contains(M::META)
        )
    };
    match data {
        D::PointerMove(p) | D::PointerDown(p) | D::PointerUp(p) | D::PointerCancel(p) | D::PointerEnter(p) | D::PointerLeave(p)
        | D::PointerOver(p) | D::PointerOut(p) | D::MouseMove(p) | D::MouseDown(p) | D::MouseUp(p) | D::MouseEnter(p)
        | D::MouseLeave(p) | D::MouseOver(p) | D::MouseOut(p) | D::TouchStart(p) | D::TouchMove(p) | D::TouchEnd(p)
        | D::TouchCancel(p) | D::Click(p) | D::ContextMenu(p) | D::DoubleClick(p) => {
            let detail = match data {
                D::DoubleClick(_) => 2,
                D::Click(_) | D::MouseDown(_) | D::MouseUp(_) => 1,
                _ => 0,
            };
            format!(
                "{{\"clientX\":{},\"clientY\":{},\"screenX\":{},\"screenY\":{},\"button\":{},\"buttons\":{},\"detail\":{},\"pointerId\":1,\"isPrimary\":true,\"pointerType\":\"mouse\",{}}}",
                p.coords.client_x,
                p.coords.client_y,
                p.coords.screen_x,
                p.coords.screen_y,
                p.button as u8,
                p.buttons.bits(),
                detail,
                mods(p.mods)
            )
        }
        D::Wheel(w) => {
            let (x, y, mode) = match w.delta {
                BlitzWheelDelta::Pixels(x, y) => (x, y, 0),
                BlitzWheelDelta::Lines(x, y) => (x, y, 1),
            };
            format!(
                "{{\"clientX\":{},\"clientY\":{},\"deltaX\":{},\"deltaY\":{},\"deltaMode\":{},\"buttons\":{},{}}}",
                w.coords.client_x,
                w.coords.client_y,
                -x,
                -y,
                mode,
                w.buttons.bits(),
                mods(w.mods)
            )
        }
        D::KeyDown(k) | D::KeyUp(k) | D::KeyPress(k) => format!(
            "{{\"key\":{},\"code\":{},\"repeat\":{},\"location\":{},{}}}",
            json_str(&k.key.to_string()),
            json_str(&k.code.to_string()),
            k.is_auto_repeating,
            k.location as u8,
            mods(k.modifiers)
        ),
        D::Input(_) => "{\"inputType\":\"insertText\"}".into(),
        _ => String::new(),
    }
}

fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for ch in s.chars() {
        match ch {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// `scheme://host[:port]` of a URL (the `localStorage` partition key).
fn origin_of(url: &str) -> String {
    url::Url::parse(url).map(|u| u.origin().ascii_serialization()).unwrap_or_default()
}

/// Per-document `DocEvent` budget before the log is compacted.
pub const MAX_DOC_EVENTS: usize = 2048;

/// A rectangle of new content pixels (tightly packed RGBA8) at `(x, y)`.
pub struct Painted {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub pixels: Vec<u8>,
}

fn paint(doc: &mut BaseDocument, w: u32, h: u32) -> Vec<u8> {
    raster(record(doc, w, h), 0, 0, w, h)
}

/// Blitz's paint commands for the viewport (cheap: ~1 ms; rasterising is
/// what costs).
fn record(doc: &mut BaseDocument, w: u32, h: u32) -> anyrender::Scene {
    let mut scene = anyrender::Scene::new();
    blitz_paint::paint_scene(&mut scene, doc, 1.0, w, h, 0, 0);
    scene
}

/// Rasterise the `(x, y, w, h)` part of `scene`.
fn raster(scene: anyrender::Scene, x: u32, y: u32, w: u32, h: u32) -> Vec<u8> {
    use anyrender::PaintScene;
    let mut painter = anyrender_vello_cpu::VelloCpuScenePainter::new(w as u16, h as u16);
    // The canvas is the page's initial containing block: browsers show white
    // where the page paints nothing, Blitz only fills html/body backgrounds.
    painter.fill(
        peniko::Fill::NonZero,
        kurbo::Affine::IDENTITY,
        peniko::Color::WHITE,
        None,
        &kurbo::Rect::new(0.0, 0.0, w as f64, h as f64),
    );
    painter.append_scene(scene, kurbo::Affine::translate((-(x as f64), -(y as f64))));
    painter.finish().data_as_u8_slice().to_vec()
}

/// Pixel rectangle `(x, y, w, h)` that can differ between two paints of the
/// same viewport, or `None` when everything must be repainted. Commands
/// shared as a common prefix and suffix paint identically; the bounds of the
/// differing middles (old and new) cover every changed pixel. A differing
/// layer push is bounded by its clip, which bounds all of the layer's
/// content; filters (blur...) and unbalanced layer changes give up.
fn damage(old: &[RenderCommand], new: &[RenderCommand], w: u32, h: u32) -> Option<(u32, u32, u32, u32)> {
    let pre = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let room = old.len().min(new.len()) - pre;
    let suf = old.iter().rev().zip(new.iter().rev()).take(room).take_while(|(a, b)| a == b).count();
    let (om, nm) = (&old[pre..old.len() - suf], &new[pre..new.len() - suf]);
    let depth = |cmds: &[RenderCommand]| -> i64 {
        cmds.iter()
            .map(|c| match c {
                RenderCommand::PushLayer(_) | RenderCommand::PushClipLayer(_) => 1,
                RenderCommand::PopLayer => -1,
                _ => 0,
            })
            .sum()
    };
    if depth(om) != depth(nm) {
        return None;
    }
    let mut r: Option<kurbo::Rect> = None;
    for c in om.iter().chain(nm) {
        if let Some(b) = command_bounds(c)? {
            r = Some(r.map_or(b, |r| r.union(b)));
        }
    }
    let Some(r) = r else { return Some((0, 0, 0, 0)) };
    // Antialiasing bleeds a pixel or so past exact geometry.
    let r = r.inflate(2.0, 2.0).expand().intersect(kurbo::Rect::new(0.0, 0.0, w as f64, h as f64));
    if r.width() <= 0.0 || r.height() <= 0.0 {
        return Some((0, 0, 0, 0));
    }
    Some((r.x0 as u32, r.y0 as u32, r.width() as u32, r.height() as u32))
}

/// Device-space bounds of what a command can touch: `Ok(None)` for nothing,
/// `None` for "unknown, repaint everything".
fn command_bounds(c: &RenderCommand) -> Option<Option<kurbo::Rect>> {
    use kurbo::Shape;
    let b = match c {
        RenderCommand::PopLayer => return Some(None),
        RenderCommand::PushLayer(l) => {
            if l.filter.is_some() || l.backdrop_filter.is_some() {
                return None;
            }
            l.transform.transform_rect_bbox(l.clip.bounding_box())
        }
        RenderCommand::PushClipLayer(l) => l.transform.transform_rect_bbox(l.clip.bounding_box()),
        RenderCommand::Fill(f) => f.transform.transform_rect_bbox(f.shape.bounding_box()),
        RenderCommand::Stroke(s) => {
            let pad = s.style.width * s.style.miter_limit.max(1.0) + 1.0;
            s.transform.transform_rect_bbox(s.shape.bounding_box().inflate(pad, pad))
        }
        RenderCommand::GlyphRun(g) => {
            if g.glyphs.is_empty() {
                return Some(None);
            }
            // Glyph origins sit on the baseline; a glyph's ink stays within
            // roughly one em around it (two with a glyph transform: skew).
            let em = g.font_size as f64 * if g.glyph_transform.is_some() { 3.0 } else { 1.5 };
            let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            for gl in &g.glyphs {
                (x0, y0) = (x0.min(gl.x as f64), y0.min(gl.y as f64));
                (x1, y1) = (x1.max(gl.x as f64), y1.max(gl.y as f64));
            }
            g.transform.transform_rect_bbox(kurbo::Rect::new(x0 - em, y0 - em, x1 + em, y1 + em))
        }
        RenderCommand::BoxShadow(s) => {
            let pad = s.radius + 3.0 * s.std_dev + 1.0;
            s.transform.transform_rect_bbox(s.rect.inflate(pad, pad))
        }
    };
    b.is_finite().then_some(Some(b))
}
