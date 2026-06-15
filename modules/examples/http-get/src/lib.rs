use crate::bindings::network::*;
use interstice_sdk::*;

// Demonstrates the *shared* network broker: instead of holding the Network
// authority itself, this module asks the default `network` module to fetch a URL
// for it. The broker resolves DNS, opens the TCP socket, sends the GET and
// assembles the response, then hands the result back as a row in its public
// `HttpResponse` table (stamped with this module's name so we filter to our own
// request). Proves multiple apps can share the single NIC through the normal
// cross-module ABI.
interstice_module!(visibility: Private);

/// This module's schema name — the broker stamps results with the caller's name,
/// so we ignore rows that belong to other apps.
const ME: &str = "http-get-example";

/// Inspectable copy of the fetched result.
#[table(public)]
#[derive(Debug)]
pub struct HttpResult {
    #[primary_key]
    pub req_id: u64,
    pub status_line: String,
    pub body_len: u64,
    pub done: bool,
}

#[reducer(on = "load")]
fn on_load(ctx: ReducerContext) {
    ctx.log("http-get: requesting http_get(example.com, /) via the network broker");
    if let Err(err) =
        ctx.network()
            .reducers
            .http_get(1, "example.com".to_string(), "/".to_string())
    {
        ctx.log(&format!("http-get: http_get call failed: {err}"));
    }
}

#[reducer(on = "network.httpresponse.insert")]
fn on_http<Caps>(ctx: ReducerContext<Caps>, row: HttpResponse)
where
    Caps: CanInsert<HttpResult>,
{
    if row.owner != ME {
        return; // belongs to another app
    }
    if !row.error.is_empty() {
        ctx.log(&format!(
            "http-get: request {} failed: {}",
            row.req_id, row.error
        ));
        return;
    }
    ctx.log(&format!(
        "http-get: req {} -> status `{}`, {} body bytes",
        row.req_id,
        row.status_line,
        row.body.len()
    ));
    let _ = ctx.current.tables.httpresult().insert(HttpResult {
        req_id: row.req_id,
        status_line: row.status_line,
        body_len: row.body.len() as u64,
        done: row.done,
    });
}
