use crate::bindings::network::*;
use interstice_sdk::*;

// Demonstrates the *shared* network broker: instead of holding the Network
// authority itself, this module asks the default `network` module to fetch a URL
// for it. The broker resolves DNS, opens the TCP socket, sends the GET and
// assembles the response, then hands the result back as a row of its private
// `HttpResponse` table, which this module reads through the `my_responses`
// view: it sees its own responses only (DESIGN §7.5). Proves multiple apps can
// share the single NIC through the normal cross-module ABI.
interstice_module!(visibility: Private);

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
    // Fetch over HTTPS: the broker terminates TLS host-side (the wasm app speaks
    // plaintext to the broker), follows any 3xx redirects, and de-chunks /
    // decompresses the body before handing it back.
    ctx.log("http-get: requesting https://example.com/");
    if let Err(err) = ctx.network().reducers.http_get(
        1,
        "example.com".to_string(),
        "/".to_string(),
        true, // tls
    ) {
        ctx.log(&format!("http-get: http_get call failed: {err}"));
    }

    // Plain HTTP that redirects to HTTPS — exercises the broker's 3xx follow +
    // scheme upgrade (http://iana.org → https://www.iana.org).
    ctx.log("http-get: requesting http://iana.org/ (redirects to https)");
    if let Err(err) = ctx.network().reducers.http_get(
        2,
        "iana.org".to_string(),
        "/".to_string(),
        false, // plain http
    ) {
        ctx.log(&format!("http-get: http_get call failed: {err}"));
    }
}

// `my_responses` shows this module its own responses only (DESIGN §7.5).
#[reducer(on = "network.my_responses.insert")]
fn on_http<Caps>(ctx: ReducerContext<Caps>, row: MyResponses)
where
    Caps: CanInsert<HttpResult>,
{
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
