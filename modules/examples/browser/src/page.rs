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

    let html = String::from_utf8_lossy(&row.body).to_string();

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
    let (links, inline_styles) = html::collect_stylesheets(&html);
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
    nav.next_req_id = next;
    nav.pending_css = pending;

    if pending == 0 {
        // No render-blocking external sheets: build now (inline `<style>` only).
        // Pass the document fields directly — the PageDoc row we just inserted is
        // NOT visible to a read in this same run (write-visibility).
        let (host, path, tls) = (nav.host.clone(), nav.path.clone(), nav.tls);
        rebuild(ctx, nav, &html, &host, &path, tls, &inline_styles);
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
        String::from_utf8_lossy(&row.body).to_string()
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
        rebuild(ctx, nav, &doc.html, &doc.host, &doc.path, doc.tls, &sheets);
    }
    let _ = ctx.current.tables.navstate().update(nav.clone());
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
    // Delete each direct child's whole subtree (flex containers nest children
    // beneath them, so a flat single-level delete would orphan grandchildren).
    let children: Vec<String> = ctx
        .current
        .tables
        .uielement()
        .scan()
        .into_iter()
        .filter(|el| el.parent.as_deref() == Some(VIEWPORT_ID))
        .map(|el| el.id)
        .collect();
    for cid in children {
        ui::delete_element(ctx, &cid);
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
