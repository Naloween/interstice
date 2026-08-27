//! HTTP-response → page pipeline: adopt the post-redirect URL, stash the document,
//! gather render-blocking stylesheets, and build (or defer the build until the
//! externals arrive). Also the previous-page teardown.

use interstice_sdk::*;

use crate::bindings::network::*;
use crate::render::rebuild;
use crate::tables::*;
use crate::ui;
use crate::ui::*;
use crate::widgets::text_el;
use crate::{html, url};

/// Handle the main document response: adopt the final URL, stash the HTML, and
/// gather the page's render-blocking stylesheets. If there are no external sheets
/// the page is built immediately; otherwise the externals are queued and the build
/// is deferred to [`rebuild`] once they arrive (CSS is render-blocking, so the page
/// is laid out exactly once — images aren't re-fetched on a later re-render).
pub(crate) fn prepare_page<Caps>(ctx: &ReducerContext<Caps>, nav: &mut NavState, row: &HttpResponse)
where
    Caps: CanRead<ui::UiElement>
        + CanInsert<ui::UiElement>
        + CanUpdate<ui::UiElement>
        + CanDelete<ui::UiElement>
        + CanRead<LinkMap>
        + CanDelete<LinkMap>
        + CanRead<ImageReq>
        + CanInsert<ImageReq>
        + CanDelete<ImageReq>
        + CanRead<ImageWaiter>
        + CanInsert<ImageWaiter>
        + CanDelete<ImageWaiter>
        + CanInsert<ImageFetchQueue>
        + CanRead<PageDoc>
        + CanInsert<PageDoc>
        + CanUpdate<PageDoc>
        + CanRead<StyleSheet>
        + CanInsert<StyleSheet>
        + CanDelete<StyleSheet>
        + CanInsert<CssFetchQueue>
        + CanRead<ScriptSheet>
        + CanInsert<ScriptSheet>
        + CanDelete<ScriptSheet>
        + CanInsert<ScriptFetchQueue>
        + CanUpdate<NavState>,
{
    // Adopt the post-redirect URL as the document base. The broker may have
    // followed one or more 3xx hops (e.g. wikipedia.com → www.wikipedia.org), and
    // relative sub-resources/links must resolve against the host that actually
    // served the page — otherwise every relative image would itself need a second
    // redirect round-trip (slow) or, if it doesn't redirect, hit the wrong host.
    if !row.final_host.is_empty()
        && (row.final_host != nav.host || row.final_path != nav.path || row.final_tls != nav.tls)
    {
        nav.host = row.final_host.clone();
        nav.path = row.final_path.clone();
        nav.tls = row.final_tls;
        nav.url = url::Location {
            host: nav.host.clone(),
            path: nav.path.clone(),
            tls: nav.tls,
        }
        .to_url();
        // Reflect the final URL in the address bar, like a real browser.
        if let Some(mut bar) = ctx.current.tables.uielement().get(URLBAR_ID.to_string()) {
            bar.text = Some(nav.url.clone());
            bar.cursor_pos = nav.url.chars().count() as u32;
            let _ = ctx.current.tables.uielement().update(bar);
        }
    }

    // A new page is being assembled: stamp its identity and discard any stylesheet
    // rows left over from the previous page (late responses for them are ignored
    // via the page_gen check in `on_css_response`).
    nav.page_gen += 1;
    let pg = nav.page_gen;
    for s in ctx.current.tables.stylesheet().scan() {
        let _ = ctx.current.tables.stylesheet().delete(s.req_id);
    }
    for s in ctx.current.tables.scriptsheet().scan() {
        let _ = ctx.current.tables.scriptsheet().delete(s.req_id);
    }

    // Consume the main request: the runtime may deliver this insert more than once
    // (we re-enter the broker mid-run); clearing it makes duplicates a no-op.
    nav.main_req_id = 0;

    if !row.error.is_empty() {
        teardown_viewport(ctx);
        nav.nav_gen += 1;
        let generation = nav.nav_gen;
        ui::create_element(
            ctx,
            text_el(
                format!("c{generation}_0"),
                VIEWPORT_ID.to_string(),
                0,
                format!("Failed to load {}: {}", nav.url, row.error),
                15.0,
                (0.90, 0.45, 0.45, 1.0),
                false,
                false,
                0.0,
                None,
                (0.0, 0.0, 0.0, 0.0),
                (0.0, 0.0, 0.0, 0.0),
                Size::Grow,
                Size::Fit,
                0.0,
                0.0,
                TRANSPARENT,
                Vec::new(),
            ),
        );
        ctx.log(&format!("browser: load error for {}: {}", nav.url, row.error));
        nav.pending_css = 0;
        let _ = ctx.current.tables.navstate().update(nav.clone());
        return;
    }

    let html = crate::encoding::decode_body(&row.body);

    // Build the persistent DOM arena for this page (the single source of truth the
    // walker/renderer read, and — from the JS phases on — that scripts mutate). It
    // lives in linear memory, so it survives to the later CSS-response rebuild.
    crate::dom::set(&html);

    // Start a fresh JS realm for this page, seed its `location`/`history` view from
    // the adopted final URL, then run its inline scripts (which may read location or
    // request a navigation — the latter is drained in `on_frame`).
    crate::js::reset();
    crate::js::set_location(&nav.url, &nav.host, &nav.path, nav.tls, nav.hist_len);
    crate::js::run_inline_scripts();

    // Stash the document so `rebuild` can re-parse it once stylesheets are ready.
    let doc = PageDoc {
        id: 0,
        html: html.clone(),
        host: nav.host.clone(),
        path: nav.path.clone(),
        tls: nav.tls,
        page_gen: pg,
    };
    if ctx.current.tables.pagedoc().insert(doc.clone()).is_err() {
        let _ = ctx.current.tables.pagedoc().update(doc);
    }

    // Collect this page's stylesheets in cascade order: external `<link>` sheets
    // (queued for fetch) first, then inline `<style>` text (immediately ready).
    let (links, inline_styles) = html::collect_stylesheets();
    let mut next = nav.next_req_id;
    let mut order = 0u32;
    let mut pending = 0u32;
    for href in &links {
        let Some(loc) = url::resolve(&nav.host, &nav.path, nav.tls, href) else {
            continue;
        };
        let rid = next;
        next += 1;
        let _ = ctx.current.tables.stylesheet().insert(StyleSheet {
            req_id: rid,
            page_gen: pg,
            order,
            css: String::new(),
            ready: false,
        });
        // Queue the fetch — DO NOT call http_get here (re-entrant; see docs).
        let _ = ctx.current.tables.cssfetchqueue().insert(CssFetchQueue {
            req_id: rid,
            host: loc.host,
            path: loc.path,
            tls: loc.tls,
        });
        pending += 1;
        order += 1;
    }
    for css_text in &inline_styles {
        let rid = next;
        next += 1;
        let _ = ctx.current.tables.stylesheet().insert(StyleSheet {
            req_id: rid,
            page_gen: pg,
            order,
            css: css_text.clone(),
            ready: true,
        });
        order += 1;
    }
    // Queue this page's external `<script src>` in document order. Unlike CSS these
    // aren't render-blocking (see `NavState.pending_js`): the page paints below on
    // CSS alone; the scripts run once they've all arrived (`on_script_response`).
    let scripts = crate::js::collect_external_scripts();
    let mut pending_js = 0u32;
    let mut sorder = 0u32;
    for src in &scripts {
        let Some(loc) = url::resolve(&nav.host, &nav.path, nav.tls, src) else {
            continue;
        };
        let rid = next;
        next += 1;
        let _ = ctx.current.tables.scriptsheet().insert(ScriptSheet {
            req_id: rid,
            page_gen: pg,
            order: sorder,
            code: String::new(),
            ready: false,
        });
        let _ = ctx.current.tables.scriptfetchqueue().insert(ScriptFetchQueue {
            req_id: rid,
            host: loc.host,
            path: loc.path,
            tls: loc.tls,
        });
        pending_js += 1;
        sorder += 1;
    }
    nav.pending_js = pending_js;

    nav.next_req_id = next;
    nav.pending_css = pending;

    if pending == 0 {
        // No render-blocking external sheets: build now (inline `<style>` only).
        // Pass the document fields directly — the PageDoc row we just inserted is
        // NOT visible to a read in this same run (write-visibility).
        let (host, path, tls) = (nav.host.clone(), nav.path.clone(), nav.tls);
        rebuild(ctx, nav, &host, &path, tls, &inline_styles);
    }
    let _ = ctx.current.tables.navstate().update(nav.clone());
}

/// Handle a completed stylesheet fetch: fill in its text, and once every
/// render-blocking sheet for this page has arrived, build the page.
pub(crate) fn on_css_response<Caps>(
    ctx: &ReducerContext<Caps>,
    nav: &mut NavState,
    row: &HttpResponse,
) where
    Caps: CanRead<ui::UiElement>
        + CanInsert<ui::UiElement>
        + CanUpdate<ui::UiElement>
        + CanDelete<ui::UiElement>
        + CanRead<LinkMap>
        + CanDelete<LinkMap>
        + CanRead<ImageReq>
        + CanInsert<ImageReq>
        + CanDelete<ImageReq>
        + CanRead<ImageWaiter>
        + CanInsert<ImageWaiter>
        + CanDelete<ImageWaiter>
        + CanInsert<ImageFetchQueue>
        + CanRead<PageDoc>
        + CanRead<StyleSheet>
        + CanUpdate<StyleSheet>
        + CanUpdate<NavState>,
{
    let Some(mut sheet) = ctx.current.tables.stylesheet().get(row.req_id) else {
        return;
    };
    // Stale: already filled, or belongs to a page the user has navigated away from.
    if sheet.ready || sheet.page_gen != nav.page_gen {
        return;
    }

    let css = if row.error.is_empty() {
        crate::encoding::decode_body(&row.body)
    } else {
        String::new() // a failed sheet just contributes nothing
    };
    sheet.css = css.clone();
    sheet.ready = true;
    let _ = ctx.current.tables.stylesheet().update(sheet);

    if nav.pending_css > 0 {
        nav.pending_css -= 1;
    }
    if nav.pending_css == 0 {
        // All render-blocking sheets are in. The just-updated row isn't visible to
        // a scan in this same run (write-visibility), so substitute its fresh text.
        // The PageDoc was stored in a prior run (prepare_page) so it reads back fine.
        let Some(doc) = ctx.current.tables.pagedoc().get(0) else {
            let _ = ctx.current.tables.navstate().update(nav.clone());
            return;
        };
        let sheets = collect_sheet_texts(ctx, nav.page_gen, row.req_id, &css);
        rebuild(ctx, nav, &doc.host, &doc.path, doc.tls, &sheets);
    }
    let _ = ctx.current.tables.navstate().update(nav.clone());
}

/// Handle a completed external-script fetch: stash its body, and once every script
/// for this page has arrived, run them in document order against the live realm.
/// Scripts typically only register event listeners (against the persistent DOM
/// arena, which survives re-renders), so a re-render happens only if a script
/// mutated the DOM.
pub(crate) fn on_script_response<Caps>(
    ctx: &ReducerContext<Caps>,
    nav: &mut NavState,
    row: &HttpResponse,
) where
    Caps: CanRead<ui::UiElement>
        + CanInsert<ui::UiElement>
        + CanUpdate<ui::UiElement>
        + CanDelete<ui::UiElement>
        + CanRead<LinkMap>
        + CanDelete<LinkMap>
        + CanRead<ImageReq>
        + CanInsert<ImageReq>
        + CanDelete<ImageReq>
        + CanRead<ImageWaiter>
        + CanInsert<ImageWaiter>
        + CanDelete<ImageWaiter>
        + CanInsert<ImageFetchQueue>
        + CanRead<ScriptSheet>
        + CanUpdate<ScriptSheet>
        + CanRead<StyleSheet>
        + CanUpdate<NavState>,
{
    let Some(mut script) = ctx.current.tables.scriptsheet().get(row.req_id) else {
        return;
    };
    // Stale: already filled, or belongs to a page the user has navigated away from.
    if script.ready || script.page_gen != nav.page_gen {
        return;
    }

    let code = if row.error.is_empty() {
        crate::encoding::decode_body(&row.body)
    } else {
        String::new() // a failed script just contributes nothing
    };
    script.code = code.clone();
    script.ready = true;
    let _ = ctx.current.tables.scriptsheet().update(script);

    if nav.pending_js > 0 {
        nav.pending_js -= 1;
    }
    if nav.pending_js == 0 {
        // All external scripts are in. Gather them in document order — the just-
        // updated row isn't visible to a scan in this same run (write-visibility),
        // so substitute its fresh body — and run them against the page's realm.
        let mut rows: Vec<ScriptSheet> = ctx
            .current
            .tables
            .scriptsheet()
            .scan()
            .into_iter()
            .filter(|s| s.page_gen == nav.page_gen)
            .collect();
        rows.sort_by_key(|s| s.order);
        let codes: Vec<String> = rows
            .into_iter()
            .map(|s| if s.req_id == row.req_id { code.clone() } else { s.code })
            .collect();
        crate::js::run_external_scripts(&codes);

        // A script that mutated the DOM (rare at load — most just add listeners)
        // requires a re-render from the live arena, exactly like a click handler.
        if crate::dom::take_dirty() {
            let (host, path, tls) = (nav.host.clone(), nav.path.clone(), nav.tls);
            let sheets = current_sheets(ctx, nav.page_gen);
            rebuild(ctx, nav, &host, &path, tls, &sheets);
        }
    }
    let _ = ctx.current.tables.navstate().update(nav.clone());
}

/// Gather the current page's stylesheet texts in cascade order (all rows already
/// stored). Used to re-render from `on_frame` after a JS event mutates the DOM.
pub(crate) fn current_sheets<Caps>(ctx: &ReducerContext<Caps>, page_gen: u32) -> Vec<String>
where
    Caps: CanRead<StyleSheet>,
{
    collect_sheet_texts(ctx, page_gen, u64::MAX, "")
}

/// Gather the current page's stylesheet texts in cascade order, substituting
/// `fresh_css` for the row `fresh_id` (whose in-run update isn't yet visible).
fn collect_sheet_texts<Caps>(
    ctx: &ReducerContext<Caps>,
    page_gen: u32,
    fresh_id: u64,
    fresh_css: &str,
) -> Vec<String>
where
    Caps: CanRead<StyleSheet>,
{
    let mut rows: Vec<StyleSheet> = ctx
        .current
        .tables
        .stylesheet()
        .scan()
        .into_iter()
        .filter(|s| s.page_gen == page_gen)
        .collect();
    rows.sort_by_key(|s| s.order);
    rows.into_iter()
        .map(|s| {
            if s.req_id == fresh_id {
                fresh_css.to_string()
            } else {
                s.css
            }
        })
        .collect()
}

/// Tear down the previous page's content: viewport children, link map, and image
/// bookkeeping, and reset the scroll to the top. All targets are committed (older
/// generation), so these are pure deletes — content inserted afterwards carries a
/// bumped generation in its ids and never collides with the rows being deleted.
pub(crate) fn teardown_viewport<Caps>(ctx: &ReducerContext<Caps>)
where
    Caps: CanRead<ui::UiElement>
        + CanUpdate<ui::UiElement>
        + CanDelete<ui::UiElement>
        + CanRead<LinkMap>
        + CanDelete<LinkMap>
        + CanRead<ImageReq>
        + CanDelete<ImageReq>
        + CanRead<ImageWaiter>
        + CanDelete<ImageWaiter>,
{
    // Delete the viewport's whole subtree. A naive recursive delete re-scans the
    // ENTIRE element table once per node to find its children — O(n²), which cost
    // ~3.3s to tear down a 906-element page (Wikipedia). Instead scan once, build a
    // parent→children map, then walk the subtree and delete every node in a single
    // pass (flex containers nest children beneath them, so we must recurse to avoid
    // orphaning grandchildren).
    let mut by_parent: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for el in ctx.current.tables.uielement().scan() {
        if let Some(p) = el.parent {
            by_parent.entry(p).or_default().push(el.id);
        }
    }
    let mut stack: Vec<String> = by_parent.remove(VIEWPORT_ID).unwrap_or_default();
    while let Some(id) = stack.pop() {
        if let Some(kids) = by_parent.remove(&id) {
            stack.extend(kids);
        }
        let _ = ctx.current.tables.uielement().delete(id);
    }
    for lm in ctx.current.tables.linkmap().scan() {
        let _ = ctx.current.tables.linkmap().delete(lm.element_id);
    }
    for r in ctx.current.tables.imagereq().scan() {
        let _ = ctx.current.tables.imagereq().delete(r.req_id);
    }
    for w in ctx.current.tables.imagewaiter().scan() {
        let _ = ctx.current.tables.imagewaiter().delete(w.element_id);
    }
    if let Some(mut vp) = ctx.current.tables.uielement().get(VIEWPORT_ID.to_string()) {
        vp.scroll_y = 0.0;
        let _ = ctx.current.tables.uielement().update(vp);
    }
}
