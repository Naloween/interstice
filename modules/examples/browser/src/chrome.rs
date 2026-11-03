//! The browser chrome (tab strip + navigation toolbar), modelled on Firefox's
//! Proton light theme. It is itself a small Blitz HTML document rendered into
//! its own texture above the page, so hover/active states, the URL text field
//! (caret, selection, editing) and layout all come from the same engine as the
//! page content.
//!
//! The document is built once; afterwards only the dynamic bits (tab list, URL
//! field, button states) are mutated in place so the URL field keeps its focus
//! and edit state.

use blitz_dom::{BaseDocument, DocumentConfig, Document, EventHandler, local_name, ns, QualName};
use blitz_html::HtmlDocument;
use blitz_traits::events::{DomEvent, DomEventData, EventState};
use blitz_traits::shell::{ColorScheme, Viewport};
use keyboard_types::{Key, NamedKey};
use std::sync::Arc;

/// Height of the tab strip.
pub const TABBAR_H: u32 = 44;
/// Height of the navigation toolbar (including its 1px bottom border).
pub const NAVBAR_H: u32 = 41;
/// Total chrome height above the page.
pub const CHROME_H: u32 = TABBAR_H + NAVBAR_H;

/// A tab as displayed in the strip.
pub struct TabView<'a> {
    pub id: u64,
    pub title: &'a str,
    pub loading: bool,
    pub selected: bool,
    pub favicon: Option<&'a str>,
}

/// Something the user did in the chrome that the browser must act on.
#[derive(Debug, Clone, PartialEq)]
pub enum ChromeAction {
    Back,
    Forward,
    /// Reload, or stop while the page is loading (the button doubles as Stop).
    Reload,
    Home,
    NewTab,
    SelectTab(u64),
    CloseTab(u64),
    /// Enter in the URL field, with the typed text.
    Go(String),
    /// The URL field gained focus (select-all, show full URL).
    FocusUrl,
    /// Escape in the URL field: revert to the page URL.
    RevertUrl,
    Menu,
    ListTabs,
}

// ── Icons ────────────────────────────────────────────────────────────────────

/// A 16x16 single-colour SVG icon as a `data:` URL usable in `<img src>`.
fn icon(body: &str) -> String {
    let svg = format!(
        "<svg xmlns='http://www.w3.org/2000/svg' width='16' height='16' viewBox='0 0 16 16' \
         fill='none' stroke='%235b5b66' stroke-width='1.25' stroke-linecap='round' \
         stroke-linejoin='round'>{body}</svg>"
    );
    // Only '#' (as %23 above) and '%' are special in a data URL payload here; the
    // rest is percent-encoded by the URL parser as needed.
    format!("data:image/svg+xml,{}", svg.replace('"', "'"))
}

fn icons() -> Vec<(&'static str, String)> {
    vec![
        ("back", icon("<path d='M13.5 8h-11M7 3.5 2.5 8 7 12.5'/>")),
        ("fwd", icon("<path d='M2.5 8h11M9 3.5 13.5 8 9 12.5'/>")),
        (
            "reload",
            icon("<path d='M13.3 6.6A5.5 5.5 0 1 0 13.5 9'/><path d='M13.6 2.5v4.1H9.5'/>"),
        ),
        ("stop", icon("<path d='M3.5 3.5l9 9M12.5 3.5l-9 9'/>")),
        (
            "home",
            icon("<path d='M2 7.5 8 2.5l6 5M3.5 6.5v7h3.5v-4h2v4h3.5v-7'/>"),
        ),
        ("plus", icon("<path d='M8 2.5v11M2.5 8h11'/>")),
        ("close", icon("<path d='M4.5 4.5l7 7M11.5 4.5l-7 7'/>")),
        ("menu", icon("<path d='M2.5 4h11M2.5 8h11M2.5 12h11'/>")),
        ("chevron", icon("<path d='M4 6.5l4 4 4-4'/>")),
        (
            "lock",
            icon("<rect x='3' y='7' width='10' height='7' rx='1.5'/><path d='M5.5 7V5a2.5 2.5 0 0 1 5 0v2'/>"),
        ),
        (
            "info",
            icon("<circle cx='8' cy='8' r='6'/><path d='M8 7.2v4M8 4.9v.1'/>"),
        ),
        (
            "search",
            icon("<circle cx='7' cy='7' r='4.5'/><path d='M10.3 10.3 14 14'/>"),
        ),
        (
            "star",
            icon("<path d='M8 1.8l1.9 3.9 4.3.6-3.1 3 .7 4.3L8 11.6l-3.8 2 .7-4.3-3.1-3 4.3-.6z'/>"),
        ),
        (
            "account",
            icon("<circle cx='8' cy='8' r='6.2'/><circle cx='8' cy='6.5' r='2.2'/><path d='M4 12.6c1-1.6 2.4-2.3 4-2.3s3 .7 4 2.3'/>"),
        ),
        (
            "ext",
            icon("<path d='M3 5.5h3V4a1.5 1.5 0 0 1 3 0v1.5h3v3h-1.5a1.5 1.5 0 0 0 0 3H12v2.5H3v-3h1.5a1.5 1.5 0 0 0 0-3H3z'/>"),
        ),
        (
            "globe",
            icon("<circle cx='8' cy='8' r='6'/><path d='M2 8h12M8 2c-2 2-2 10 0 12M8 2c2 2 2 10 0 12'/>"),
        ),
        (
            "loading",
            "data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' width='16' height='16' viewBox='0 0 16 16'><circle cx='8' cy='8' r='6' fill='none' stroke='%23d7d7db' stroke-width='2'/><path d='M8 2a6 6 0 0 1 6 6' fill='none' stroke='%230061e0' stroke-width='2' stroke-linecap='round'/></svg>".to_string(),
        ),
    ]
}

thread_local! {
    static ICONS: Vec<(&'static str, String)> = icons();
}

pub fn icon_url(name: &str) -> String {
    ICONS.with(|i| {
        i.iter()
            .find(|(n, _)| *n == name)
            .map(|(_, u)| u.clone())
            .unwrap_or_default()
    })
}

// ── Document ─────────────────────────────────────────────────────────────────

const CSS: &str = r#"
html, body { margin:0; padding:0; overflow:hidden; background:#f0f0f4; color:#15141a;
  font-family:"Liberation Sans", sans-serif; font-size:12px; cursor:default; user-select:none; }
#tabbar { height:44px; display:flex; align-items:center; padding:0 2px 0 2px; box-sizing:border-box; }
#tabs { display:flex; flex:0 1 auto; min-width:0; height:44px; align-items:center; overflow:hidden; }
.tab { flex:0 1 225px; width:225px; min-width:76px; height:36px; margin:0 2px; border-radius:4px;
  display:flex; align-items:center; padding:0 4px 0 9px; box-sizing:border-box; position:relative; }
.tab:hover { background:rgba(207,207,216,0.45); }
.tab.sel { background:#ffffff; box-shadow:0 0 4px rgba(0,0,0,0.25); }
.fav { width:16px; height:16px; margin-right:6px; flex:none; }
.lbl { flex:1 1 auto; min-width:0; overflow:hidden; white-space:nowrap; line-height:16px; }
.x { width:24px; height:24px; border-radius:4px; flex:none; display:flex; align-items:center; justify-content:center; }
.x:hover { background:rgba(207,207,216,0.66); }
.x:active { background:rgb(207,207,216); }
.x img { width:12px; height:12px; }
.tbtn { width:32px; height:32px; border-radius:4px; flex:none; display:flex; align-items:center; justify-content:center; margin:0 1px; }
.tbtn:hover { background:rgba(207,207,216,0.66); }
.tbtn:active { background:rgb(207,207,216); }
.tbtn img { width:16px; height:16px; }
.tbtn.dis:hover, .tbtn.dis:active { background:transparent; }
.tbtn.dis img { opacity:0.4; }
#newtab { margin-left:2px; }
#spring { flex:1 1 auto; height:44px; }
#nav { height:40px; background:#f9f9fb; display:flex; align-items:center; padding:0 4px; border-bottom:1px solid #d7d7db; }
.space { flex:1 1 0; min-width:4px; }
#urlwrap { flex:16 1 0; min-width:200px; height:32px; background:#f0f0f4; border-radius:4px; display:flex;
  align-items:center; padding:0 2px 0 4px; box-sizing:border-box; border:1px solid transparent; margin:0 2px; }
#urlwrap:hover { background:#e6e6ea; }
#urlwrap.focus { background:#ffffff; border:2px solid #0061e0; padding:0 1px 0 3px; }
#ident { width:24px; height:24px; border-radius:4px; flex:none; display:flex; align-items:center; justify-content:center; margin-right:4px; }
#ident img { width:16px; height:16px; }
#urlbar { flex:1 1 auto; min-width:0; border:0; background:transparent; font-family:"Liberation Sans", sans-serif;
  font-size:14px; color:#15141a; padding:0; margin:0; height:22px; outline:none; }
#urldisp { flex:1 1 auto; min-width:0; overflow:hidden; white-space:nowrap; font-size:14px; line-height:22px; }
#urldisp .host { color:#15141a; }
#urldisp .rest { color:#5b5b66; }
#urldisp .ph { color:#5b5b66; }
#star { width:28px; height:24px; }
.hidden { display:none; }
"#;

pub fn build(width: u32) -> HtmlDocument {
    let ic = |n: &str| icon_url(n);
    let html = format!(
        r#"<!DOCTYPE html><html><head><style>{CSS}</style></head><body>
<div id="tabbar"><div id="tabs"></div>
<div class="tbtn" id="newtab"><img src="{plus}"></div>
<div id="spring"></div>
<div class="tbtn" id="alltabs"><img src="{chevron}"></div></div>
<div id="nav">
<div class="tbtn dis" id="back"><img src="{back}"></div>
<div class="tbtn dis" id="fwd"><img src="{fwd}"></div>
<div class="tbtn" id="reload"><img id="reloadimg" src="{reload}"></div>
<div class="tbtn" id="home"><img src="{home}"></div>
<div class="space"></div>
<div id="urlwrap"><div id="ident"><img id="identimg" src="{search}"></div>
<div id="urldisp"><span class="ph">Search or enter address</span></div>
<input id="urlbar" class="hidden" type="text" value="">
<div class="tbtn" id="star"><img src="{star}"></div></div>
<div class="space"></div>
<div class="tbtn" id="account"><img src="{account}"></div>
<div class="tbtn" id="ext"><img src="{ext}"></div>
<div class="tbtn" id="menu"><img src="{menu}"></div>
</div></body></html>"#,
        plus = ic("plus"),
        chevron = ic("chevron"),
        back = ic("back"),
        fwd = ic("fwd"),
        reload = ic("reload"),
        home = ic("home"),
        search = ic("search"),
        star = ic("star"),
        account = ic("account"),
        ext = ic("ext"),
        menu = ic("menu"),
    );
    let cfg = DocumentConfig {
        viewport: Some(Viewport::new(width, CHROME_H, 1.0, ColorScheme::Light)),
        base_url: Some("about:chrome".into()),
        net_provider: Some(Arc::new(crate::providers::Net)),
        shell_provider: Some(Arc::new(crate::providers::ChromeShell)),
        html_parser_provider: Some(Arc::new(blitz_html::HtmlProvider)),
        font_ctx: Some(crate::fonts::font_context()),
        ..Default::default()
    };
    HtmlDocument::from_html(&html, cfg)
}

fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            _ => o.push(c),
        }
    }
    o
}

fn qual(name: &str) -> QualName {
    QualName::new(None, ns!(), name.into())
}

/// Replace the tab list.
pub fn set_tabs(doc: &mut BaseDocument, tabs: &[TabView]) {
    let Some(node) = doc.get_element_by_id("tabs") else {
        return;
    };
    let mut html = String::new();
    for t in tabs {
        let fav = if t.loading {
            icon_url("loading")
        } else {
            t.favicon.map(str::to_string).unwrap_or_else(|| icon_url("globe"))
        };
        let title = if t.title.is_empty() { "New Tab" } else { t.title };
        html.push_str(&format!(
            r#"<div class="tab{sel}" id="tab-{id}"><img class="fav" src="{fav}"><div class="lbl">{title}</div><div class="x" id="close-{id}"><img src="{close}"></div></div>"#,
            sel = if t.selected { " sel" } else { "" },
            id = t.id,
            fav = escape(&fav),
            title = escape(title),
            close = icon_url("close"),
        ));
    }
    doc.mutate().set_inner_html(node, &html);
}

/// Update toolbar button states.
pub fn set_nav_state(doc: &mut BaseDocument, can_back: bool, can_fwd: bool, loading: bool) {
    let back = doc.get_element_by_id("back");
    let fwd = doc.get_element_by_id("fwd");
    let img = doc.get_element_by_id("reloadimg");
    let mut m = doc.mutate();
    if let Some(n) = back {
        m.set_attribute(n, qual("class"), if can_back { "tbtn" } else { "tbtn dis" });
    }
    if let Some(n) = fwd {
        m.set_attribute(n, qual("class"), if can_fwd { "tbtn" } else { "tbtn dis" });
    }
    if let Some(n) = img {
        m.set_attribute(n, qual("src"), &icon_url(if loading { "stop" } else { "reload" }));
    }
}

/// Show the page URL in the (unfocused) URL bar, Firefox style: scheme hidden for
/// http(s), host emphasised, the rest greyed.
pub fn set_url_display(doc: &mut BaseDocument, url: &str) {
    let disp = doc.get_element_by_id("urldisp");
    let ident = doc.get_element_by_id("identimg");
    let mut html = String::new();
    let mut ident_icon = "search";
    if url.is_empty() || url.starts_with("about:newtab") || url.starts_with("about:home") {
        html.push_str(r#"<span class="ph">Search or enter address</span>"#);
    } else if let Ok(u) = url::Url::parse(url) {
        match u.scheme() {
            "https" | "http" => {
                ident_icon = if u.scheme() == "https" { "lock" } else { "info" };
                let host = u.host_str().unwrap_or("");
                let full = u.as_str();
                let after_scheme = full.splitn(2, "://").nth(1).unwrap_or(full);
                let rest = after_scheme.strip_prefix(host).unwrap_or("");
                let prefix = if u.scheme() == "http" { "" } else { "" };
                html.push_str(&format!(
                    r#"<span class="rest">{}</span><span class="host">{}</span><span class="rest">{}</span>"#,
                    prefix,
                    escape(host),
                    escape(rest)
                ));
            }
            _ => {
                ident_icon = "info";
                html.push_str(&format!(r#"<span class="host">{}</span>"#, escape(url)));
            }
        }
    } else {
        html.push_str(&format!(r#"<span class="host">{}</span>"#, escape(url)));
    }
    let mut m = doc.mutate();
    if let Some(n) = disp {
        m.set_inner_html(n, &html);
    }
    if let Some(n) = ident {
        m.set_attribute(n, qual("src"), &icon_url(ident_icon));
    }
}

/// Switch the URL field between its display form and the editable input.
pub fn set_url_editing(doc: &mut BaseDocument, editing: bool, value: &str) {
    let wrap = doc.get_element_by_id("urlwrap");
    let disp = doc.get_element_by_id("urldisp");
    let input = doc.get_element_by_id("urlbar");
    {
        let mut m = doc.mutate();
        if let Some(n) = wrap {
            m.set_attribute(n, qual("class"), if editing { "focus" } else { "" });
        }
        if let Some(n) = disp {
            m.set_attribute(n, qual("class"), if editing { "hidden" } else { "" });
        }
        if let Some(n) = input {
            m.set_attribute(n, qual("class"), if editing { "" } else { "hidden" });
            m.set_attribute(n, qual("value"), value);
        }
    }
    if let Some(n) = input {
        if editing {
            doc.set_focus_to(n);
            doc.with_text_input(n, |mut d| d.select_all());
        } else {
            doc.clear_focus();
        }
    }
}

/// Current text of the URL field.
pub fn url_text(doc: &BaseDocument) -> String {
    doc.get_element_by_id("urlbar")
        .and_then(|n| doc.get_node(n))
        .and_then(|n| n.element_data())
        .and_then(|e| e.text_input_data())
        .map(|t| t.editor.text().to_string())
        .unwrap_or_default()
}

/// Event handler for the chrome document: maps clicks / Enter / Escape to
/// [`ChromeAction`]s by element id.
pub struct ChromeHandler<'a> {
    pub actions: &'a mut Vec<ChromeAction>,
}

impl EventHandler for ChromeHandler<'_> {
    fn handle_event(
        &mut self,
        chain: &[blitz_dom::NodeId],
        event: &mut DomEvent,
        doc: &mut dyn Document,
        state: &mut EventState,
    ) {
        match &event.data {
            DomEventData::Click(_) => {
                let doc = doc.inner();
                for &id in chain {
                    let Some(el_id) = doc.get_node(id).and_then(|n| n.attr(local_name!("id"))) else {
                        continue;
                    };
                    let action = match el_id {
                        "back" => Some(ChromeAction::Back),
                        "fwd" => Some(ChromeAction::Forward),
                        "reload" => Some(ChromeAction::Reload),
                        "home" => Some(ChromeAction::Home),
                        "newtab" => Some(ChromeAction::NewTab),
                        "menu" => Some(ChromeAction::Menu),
                        "alltabs" => Some(ChromeAction::ListTabs),
                        "urlwrap" | "urldisp" | "ident" | "urlbar" => {
                            // Clicks inside the live input just move the caret.
                            if el_id == "urlbar" {
                                return;
                            }
                            Some(ChromeAction::FocusUrl)
                        }
                        s if s.starts_with("close-") => {
                            s[6..].parse().ok().map(ChromeAction::CloseTab)
                        }
                        s if s.starts_with("tab-") => s[4..].parse().ok().map(ChromeAction::SelectTab),
                        _ => None,
                    };
                    if let Some(a) = action {
                        self.actions.push(a);
                        return;
                    }
                }
            }
            DomEventData::KeyDown(k) => match &k.key {
                Key::Named(NamedKey::Enter) => {
                    let text = url_text(&doc.inner());
                    self.actions.push(ChromeAction::Go(text));
                    state.prevent_default();
                }
                Key::Named(NamedKey::Escape) => {
                    self.actions.push(ChromeAction::RevertUrl);
                    state.prevent_default();
                }
                _ => {}
            },
            _ => {}
        }
    }
}
