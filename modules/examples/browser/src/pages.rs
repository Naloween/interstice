//! Built-in `about:` pages, in the Interstice brand (brand/BRAND.md §8):
//! these are the browser's own pages, so unlike web content they are themed.

/// Search engine used for non-URL input in the address bar and the new-tab
/// search box. DuckDuckGo's HTML endpoint works without JavaScript over GET.
pub const SEARCH_URL: &str = "https://html.duckduckgo.com/html/?q=";

pub fn search_url(query: &str) -> String {
    let mut q = String::new();
    for b in query.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => q.push(b as char),
            b' ' => q.push('+'),
            _ => q.push_str(&format!("%{b:02X}")),
        }
    }
    format!("{SEARCH_URL}{q}")
}

const SHORTCUTS: &[(&str, &str)] = &[
    ("Wikipedia", "https://en.wikipedia.org/wiki/Main_Page"),
    ("Hacker News", "https://news.ycombinator.com/"),
    ("Rust", "https://www.rust-lang.org/"),
    ("MDN", "https://developer.mozilla.org/en-US/"),
    ("Example", "https://example.com/"),
    ("Lobsters", "https://lobste.rs/"),
    ("Old Reddit", "https://old.reddit.com/"),
    ("GitHub", "https://github.com/"),
];

/// The slit (brand/mark.svg) for the new-tab hero: a core lens with a
/// gradient, an amber glow lens and a soft bloom, all flat gradients (no blur
/// filter), as a data URL for an `<img>`.
const HERO_SLIT: &str = "data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' width='40' height='120' viewBox='0 0 40 120'><defs><linearGradient id='c' x1='0' y1='0' x2='0' y2='1'><stop offset='0' stop-color='%23c59b60' stop-opacity='0'/><stop offset='0.22' stop-color='%23e8c894'/><stop offset='0.5' stop-color='%23f6e9d2'/><stop offset='0.78' stop-color='%23e8c894'/><stop offset='1' stop-color='%23c59b60' stop-opacity='0'/></linearGradient><linearGradient id='h' x1='0' y1='0' x2='1' y2='0'><stop offset='0' stop-color='%23c59b60' stop-opacity='0'/><stop offset='0.5' stop-color='%23c59b60' stop-opacity='0.55'/><stop offset='1' stop-color='%23c59b60' stop-opacity='0'/></linearGradient><radialGradient id='b' cx='0.5' cy='0.5' r='0.5'><stop offset='0' stop-color='%23c59b60' stop-opacity='0.22'/><stop offset='1' stop-color='%23c59b60' stop-opacity='0'/></radialGradient></defs><ellipse cx='20' cy='60' rx='18' ry='56' fill='url(%23b)'/><path d='M20 6Q27 60 20 114Q13 60 20 6Z' fill='url(%23h)'/><path d='M20 4Q22.2 60 20 116Q17.8 60 20 4Z' fill='url(%23c)'/></svg>";

pub fn newtab() -> String {
    let mut tiles = String::new();
    for (name, url) in SHORTCUTS {
        let letter = name.chars().next().unwrap_or('?');
        let host = url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
        tiles.push_str(&format!(
            r#"<a class="tile" href="{url}"><div class="icon">{letter}</div><div class="name">{name}</div><div class="host">{host}</div></a>"#
        ));
    }
    format!(
        r#"<!DOCTYPE html><html><head><title>New Tab</title><style>
html,body{{margin:0;background:#151618;color:#E8E9E6;font-family:"Space Grotesk",sans-serif;}}
.wrap{{max-width:760px;margin:0 auto;padding-top:96px;text-align:center;}}
.logo{{display:flex;align-items:center;justify-content:center;height:120px;}}
.slit{{width:40px;height:120px;margin-right:6px;}}
.word{{font-size:44px;font-weight:400;letter-spacing:0.32em;margin-right:-0.32em;color:#E8E9E6;line-height:44px;}}
.tag{{font-size:13px;letter-spacing:0.22em;color:#747B82;margin:10px 0 44px;}}
form{{margin:0 auto 56px;width:560px;}}
.search{{display:flex;align-items:center;height:44px;background:#0F1012;border-radius:6px;
  border:1px solid rgba(232,233,230,0.12);padding:0 16px;box-sizing:border-box;}}
.search:hover{{border:1px solid rgba(232,233,230,0.2);}}
.search input{{flex:1;border:0;font-size:15px;height:30px;outline:none;background:transparent;color:#E8E9E6;
  caret-color:#C59B60;font-family:"Space Grotesk",sans-serif;}}
.label{{display:flex;align-items:center;font-size:11px;letter-spacing:0.18em;color:#747B82;margin:0 0 18px;text-align:left;}}
.label .rule{{flex:1;height:1px;background:#2C2F35;margin-left:14px;}}
.tiles{{display:flex;flex-wrap:wrap;justify-content:space-between;}}
.tile{{width:178px;height:64px;margin:0 0 12px;text-decoration:none;color:#E8E9E6;display:block;position:relative;
  background:#212429;border:1px solid #2C2F35;border-radius:10px;box-sizing:border-box;text-align:left;}}
.tile:hover{{border:1px solid rgba(197,155,96,0.35);}}
.icon{{position:absolute;left:12px;top:12px;width:38px;height:38px;border-radius:8px;background:#181B1F;
  border:1px solid #2C2F35;box-sizing:border-box;color:#E8E9E6;font-size:16px;line-height:36px;text-align:center;}}
.name{{position:absolute;left:62px;right:10px;top:14px;font-size:13px;font-weight:500;white-space:nowrap;overflow:hidden;}}
.host{{position:absolute;left:62px;right:10px;top:34px;font-size:11px;color:#747B82;font-family:"IBM Plex Mono",monospace;white-space:nowrap;overflow:hidden;}}
</style></head><body><div class="wrap">
<div class="logo"><img class="slit" src="{slit}"><div class="word">NTERSTICE</div></div>
<div class="tag">A SPACE FOR THINGS TO INTERACT</div>
<form action="https://html.duckduckgo.com/html/" method="get"><div class="search">
<input name="q" type="text" placeholder="Search with DuckDuckGo or enter address" autofocus></div></form>
<div class="label">SHORTCUTS<div class="rule"></div></div>
<div class="tiles">{tiles}</div></div></body></html>"#,
        slit = HERO_SLIT
    )
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

pub fn error(url: &str, err: &str) -> String {
    let host = url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_else(|| url.to_string());
    let (title, msg) = if err.contains("resolve") || err.contains("dns") || err.contains("lookup") {
        (
            "Hmm. We’re having trouble finding that site.".to_string(),
            format!("We can’t connect to the server at {}.", escape(&host)),
        )
    } else {
        (
            "Unable to connect".to_string(),
            format!("The connection to {} failed: {}", escape(&host), escape(err)),
        )
    };
    format!(
        r#"<!DOCTYPE html><html><head><title>Problem loading page</title><style>
html,body{{margin:0;background:#151618;color:#E8E9E6;font-family:"Space Grotesk",sans-serif;font-size:15px;}}
.box{{max-width:640px;margin:0 auto;padding:120px 0 0 0;}}
.kicker{{font-size:11px;letter-spacing:0.18em;color:#D0796B;margin:0 0 14px;}}
h1{{font-size:24px;font-weight:400;margin:0 0 20px;border-left:0;}}
p{{line-height:1.55;margin:0 0 12px;color:#8A9097;}} b{{color:#E8E9E6;font-weight:500;}}
ul{{margin:0 0 28px;padding-left:20px;line-height:1.6;color:#8A9097;}}
.btn{{display:inline-block;background:#C59B60;color:#0F1012;border-radius:6px;padding:9px 18px;text-decoration:none;font-weight:500;font-size:13px;}}
.btn:hover{{background:#DDB97F;}}
</style></head><body><div class="box"><div class="kicker">PROBLEM LOADING PAGE</div><h1>{title}</h1><p>{msg}</p>
<p><b>If you entered the right address, you can:</b></p>
<ul><li>Try again later</li><li>Check your network connection</li>
<li>Check that the address is spelled correctly</li></ul>
<a class="btn" href="{url}">Try Again</a></div></body></html>"#,
        url = escape(url)
    )
}

/// Wrap a non-HTML response so it still displays.
pub fn image_page(url: &str) -> String {
    format!(
        r#"<!DOCTYPE html><html><head><style>html,body{{margin:0;height:100%;background:#222;}}
body{{display:flex;align-items:center;justify-content:center;}}img{{max-width:100%;max-height:100%;}}</style></head>
<body><img src="{}"></body></html>"#,
        escape(url)
    )
}

pub fn text_page(text: &str) -> String {
    format!(
        r#"<!DOCTYPE html><html><head></head><body><pre style="white-space:pre-wrap;word-wrap:break-word;">{}</pre></body></html>"#,
        escape(text)
    )
}
