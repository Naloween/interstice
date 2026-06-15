use interstice_sdk::*;

// This module *holds* the Network authority itself — the smallest faithful
// end-to-end proof of the raw-socket authority (like the input module holding
// `Input`). It performs a plain HTTP/1.1 GET over port 80 against a hard-coded
// IP (DNS is out of scope for the authority) and assembles the response into a
// table so the outcome is inspectable via logs.
interstice_module!(visibility: Private, authorities: [Network]);

// Hard-coded target. example.com is currently served by Cloudflare; these are
// stable anycast addresses. DNS resolution would be a library layered on the
// UDP authority later — it is deliberately not part of this raw test.
const TARGET_IP: &str = "104.20.23.154";
const TARGET_HOST: &str = "example.com";
const TARGET_PORT: u16 = 80;

// TABLES

/// Accumulates the bytes of the HTTP response for the single in-flight request.
/// `status_line` is filled once the first line of the response is parsed;
/// `body` holds the raw assembled response text; `done` flips once the peer
/// closes the connection.
#[table(public)]
#[derive(Debug)]
pub struct HttpResult {
    #[primary_key]
    pub id: u64,
    pub handle: u64,
    pub status_line: String,
    pub body: String,
    pub done: bool,
}

// REDUCERS

#[reducer(on = "load")]
fn on_load<Caps>(ctx: ReducerContext<Caps>)
where
    Caps: CanInsert<HttpResult>,
{
    ctx.log(&format!(
        "http-get: connecting to {}:{} (Host: {})",
        TARGET_IP, TARGET_PORT, TARGET_HOST
    ));

    let handle = match tcp_connect(TARGET_IP.to_string(), TARGET_PORT) {
        Ok(handle) => {
            ctx.log(&format!("http-get: connect registered, handle = {}", handle));
            handle
        }
        Err(err) => {
            ctx.log(&format!("http-get: tcp_connect failed: {}", err));
            return;
        }
    };

    let row = HttpResult {
        id: 0,
        handle,
        status_line: String::new(),
        body: String::new(),
        done: false,
    };
    if let Err(err) = ctx.current.tables.httpresult().insert(row) {
        ctx.log(&format!("http-get: failed to init HttpResult: {}", err));
    }
}

#[reducer(on = "network")]
fn on_network<Caps>(ctx: ReducerContext<Caps>, event: NetworkEvent)
where
    Caps: CanRead<HttpResult> + CanUpdate<HttpResult>,
{
    match event {
        NetworkEvent::Connected { handle } => {
            ctx.log(&format!("http-get: Connected (handle {})", handle));
            let request = format!(
                "GET / HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
                TARGET_HOST
            );
            if let Err(err) = tcp_send(handle, request.into_bytes()) {
                ctx.log(&format!("http-get: tcp_send failed: {}", err));
            } else {
                ctx.log("http-get: GET request sent");
            }
        }
        NetworkEvent::ConnectFailed { handle, error } => {
            ctx.log(&format!(
                "http-get: ConnectFailed (handle {}): {}",
                handle, error
            ));
        }
        NetworkEvent::Received { handle, data } => {
            let chunk = String::from_utf8_lossy(&data);
            ctx.log(&format!(
                "http-get: Received {} bytes on handle {}",
                data.len(),
                handle
            ));
            if let Some(mut row) = ctx.current.tables.httpresult().get(0) {
                if row.status_line.is_empty() {
                    let combined = format!("{}{}", row.body, chunk);
                    if let Some(line_end) = combined.find("\r\n") {
                        row.status_line = combined[..line_end].to_string();
                        ctx.log(&format!("http-get: status line: {}", row.status_line));
                    }
                }
                row.body.push_str(&chunk);
                let _ = ctx.current.tables.httpresult().update(row);
            }
        }
        NetworkEvent::Closed { handle } => {
            ctx.log(&format!("http-get: Closed (handle {})", handle));
            if let Some(mut row) = ctx.current.tables.httpresult().get(0) {
                row.done = true;
                let total = row.body.len();
                let status = row.status_line.clone();
                let _ = ctx.current.tables.httpresult().update(row);
                ctx.log(&format!(
                    "http-get: done — status `{}`, {} total bytes",
                    status, total
                ));
            }
        }
        NetworkEvent::Failed { handle, error } => {
            ctx.log(&format!("http-get: Failed (handle {}): {}", handle, error));
        }
        // This module only opens a client connection; listener/UDP events are
        // not expected here.
        NetworkEvent::Accepted { .. } => {}
        NetworkEvent::UdpReceived { .. } => {}
    }
}
