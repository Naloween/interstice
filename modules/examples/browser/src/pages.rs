//! Built-in `about:` pages, styled after Firefox's own.

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

const SHORTCUTS: &[(&str, &str, &str)] = &[
    ("Wikipedia", "https://en.wikipedia.org/wiki/Main_Page", "#3a3a3a"),
    ("Hacker News", "https://news.ycombinator.com/", "#ff6600"),
    ("Rust", "https://www.rust-lang.org/", "#000000"),
    ("MDN", "https://developer.mozilla.org/en-US/", "#1b1b1b"),
    ("Example", "https://example.com/", "#5b5b66"),
    ("Lobsters", "https://lobste.rs/", "#ac130d"),
    ("Old Reddit", "https://old.reddit.com/", "#ff4500"),
    ("GitHub", "https://github.com/", "#24292f"),
];

pub fn newtab() -> String {
    let mut tiles = String::new();
    for (name, url, color) in SHORTCUTS {
        let letter = name.chars().next().unwrap_or('?');
        tiles.push_str(&format!(
            r#"<a class="tile" href="{url}"><div class="icon" style="background:{color}">{letter}</div><div class="name">{name}</div></a>"#
        ));
    }
    format!(
        r#"<!DOCTYPE html><html><head><title>New Tab</title><style>
html,body{{margin:0;background:#f9f9fb;color:#15141a;font-family:"Liberation Sans",sans-serif;}}
.wrap{{max-width:760px;margin:0 auto;padding-top:110px;text-align:center;}}
.logo{{display:flex;align-items:center;justify-content:center;margin-bottom:40px;}}
.mark{{width:72px;height:72px;border-radius:36px;background:linear-gradient(135deg,#ff980e,#ff3750 60%,#9059ff);
  display:flex;align-items:center;justify-content:center;}}
.mark div{{width:44px;height:44px;border-radius:22px;background:#f9f9fb;}}
.word{{font-size:44px;font-weight:bold;margin-left:16px;color:#20123a;}}
form{{margin:0 auto 48px;width:720px;}}
.search{{display:flex;align-items:center;height:52px;background:#fff;border-radius:8px;
  box-shadow:0 3px 8px rgba(0,0,0,0.08),0 0 2px rgba(0,0,0,0.3);padding:0 16px;}}
.search input{{flex:1;border:0;font-size:15px;height:30px;outline:none;background:transparent;font-family:"Liberation Sans",sans-serif;}}
.tiles{{display:flex;flex-wrap:wrap;justify-content:center;}}
.tile{{width:120px;margin:0 0 24px;text-decoration:none;color:#15141a;display:block;}}
.icon{{width:80px;height:80px;margin:0 auto 8px;border-radius:8px;color:#fff;font-size:34px;line-height:80px;
  box-shadow:0 2px 6px rgba(0,0,0,0.15);font-weight:bold;}}
.tile:hover .icon{{box-shadow:0 0 0 3px #e0e0e6,0 2px 6px rgba(0,0,0,0.2);}}
.name{{font-size:12px;}}
</style></head><body><div class="wrap">
<div class="logo"><div class="mark"><div></div></div><div class="word">Interstice</div></div>
<form action="https://html.duckduckgo.com/html/" method="get"><div class="search">
<input name="q" type="text" placeholder="Search with DuckDuckGo or enter address" autofocus></div></form>
<div class="tiles">{tiles}</div></div></body></html>"#
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
html,body{{margin:0;background:#f9f9fb;color:#15141a;font-family:"Liberation Sans",sans-serif;font-size:15px;}}
.box{{max-width:640px;margin:0 auto;padding:120px 0 0 0;}}
h1{{font-size:28px;font-weight:300;margin:0 0 24px;border-left:0;}}
p{{line-height:1.5;margin:0 0 12px;}} ul{{margin:0 0 24px;padding-left:20px;line-height:1.6;}}
.btn{{display:inline-block;background:#0061e0;color:#fff;border-radius:4px;padding:8px 16px;text-decoration:none;font-weight:bold;}}
.btn:hover{{background:#0250bb;}}
</style></head><body><div class="box"><h1>{title}</h1><p>{msg}</p>
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
