//! Navigation primitives: issuing a fetch, starting a fresh navigation (with
//! history push), and re-fetching the current history entry after a back/forward
//! step. None of these touch UI — they mutate the caller's `NavState` in place and
//! kick the broker; the caller writes `NavState` back.

use interstice_sdk::*;

use crate::bindings::network::*;
use crate::tables::*;
use crate::url;

/// Fetch `loc`: stamp the new host/path/url, allocate a request id, bump the
/// generation, and ask the broker to fetch it. Mutates the caller's NavState in
/// place (written back once by the caller). Does NOT touch history — callers
/// decide whether this is a fresh navigation (push) or a back/forward (no push).
fn issue_fetch<Caps>(ctx: &ReducerContext<Caps>, nav: &mut NavState, loc: url::Location) {
    let req_id = nav.next_req_id;
    nav.next_req_id += 1;
    nav.main_req_id = req_id;
    nav.nav_gen += 1;
    nav.host = loc.host.clone();
    nav.path = loc.path.clone();
    nav.tls = loc.tls;
    nav.url = loc.to_url();
    if let Err(err) = ctx
        .network()
        .reducers
        .http_get(req_id, loc.host, loc.path, loc.tls)
    {
        ctx.log(&format!("browser: http_get failed: {err}"));
    }
}

/// A fresh navigation to `raw_url`: parse it, push it onto the history stack
/// (truncating any forward entries), then fetch it.
pub(crate) fn start_navigation<Caps>(ctx: &ReducerContext<Caps>, nav: &mut NavState, raw_url: &str)
where
    Caps: CanRead<History> + CanInsert<History> + CanDelete<History>,
{
    let Some(loc) = url::parse(raw_url) else {
        ctx.log(&format!("browser: cannot parse url `{raw_url}`"));
        return;
    };

    // Truncate the forward history: once you navigate somewhere new, the pages you
    // had gone "back" from are gone. The new entry lands right after the current
    // position (or at 0 for the very first navigation).
    let new_idx = if nav.hist_len == 0 { 0 } else { nav.hist_pos + 1 };
    for idx in new_idx..nav.hist_len {
        let _ = ctx.current.tables.history().delete(idx);
    }
    let _ = ctx.current.tables.history().insert(History {
        idx: new_idx,
        url: loc.to_url(),
        host: loc.host.clone(),
        path: loc.path.clone(),
        tls: loc.tls,
    });
    nav.hist_pos = new_idx;
    nav.hist_len = new_idx + 1;

    issue_fetch(ctx, nav, loc);
}

/// Re-fetch the history entry at `nav.hist_pos` after it's been moved by a
/// back/forward step. Returns the entry's URL so the caller can update the bar.
pub(crate) fn load_current_history<Caps>(
    ctx: &ReducerContext<Caps>,
    nav: &mut NavState,
) -> Option<String>
where
    Caps: CanRead<History>,
{
    let entry = ctx.current.tables.history().get(nav.hist_pos)?;
    let loc = url::Location {
        host: entry.host.clone(),
        path: entry.path.clone(),
        tls: entry.tls,
    };
    issue_fetch(ctx, nav, loc);
    Some(entry.url)
}
