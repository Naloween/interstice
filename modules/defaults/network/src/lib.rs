use interstice_sdk::*;

mod dns;

// The default network broker. It holds the single `Network` authority for the
// node and re-exposes it to co-located apps through the normal cross-module
// table/reducer ABI (the same way `graphics` shares the Gpu and `module_manager`
// shares the Module authority). Three layers, smallest → highest level:
//   1. raw TCP    — connect / send / close
//   2. DNS        — resolve a hostname to an IPv4 (UDP/53)
//   3. HTTP GET   — host → DNS → TCP → HTTP/1.1 GET → assembled response
//
// Everything is async: an app calls a reducer with its own `req_id`, then reads
// the result from a public table (stamped with `owner` = the calling module, so
// each app filters to its own rows — proper per-app visibility waits on table
// views, see the README TODO).
interstice_module!(visibility: Public, authorities: [Network]);

/// Public resolver used for DNS queries (Cloudflare). Hard-coded: choosing a
/// resolver is policy, not part of the raw authority.
const DNS_SERVER: &str = "1.1.1.1";
const DNS_PORT: u16 = 53;
const HTTP_PORT: u16 = 80;
const HTTPS_PORT: u16 = 443;
/// How many `3xx Location` hops a single HTTP job will follow before giving up.
/// Guards against redirect loops (e.g. a misconfigured site bouncing forever).
const MAX_REDIRECTS: u32 = 10;

// ── Internal bookkeeping (ephemeral; not meant for apps) ────────────────────────

/// Singleton (id = 0): the bound DNS UDP socket handle + a rolling 16-bit txid.
#[table(ephemeral)]
pub struct NetCfg {
    #[primary_key]
    id: u64,
    dns_handle: u64,
    next_txid: u64,
}

/// One row per live TCP connection, so socket events route back to the owner.
/// `kind` 0 = raw (app drives it), 1 = http (the broker drives it).
#[table(ephemeral)]
pub struct TcpPending {
    #[primary_key]
    handle: u64,
    owner: String,
    req_id: u64,
    kind: u32,
    job_txid: u64,
}

/// In-flight DNS query, keyed by transaction id. `is_http` selects the result
/// path: false → emit a `Resolved` row; true → continue an HTTP job.
#[table(ephemeral)]
pub struct DnsPending {
    #[primary_key]
    txid: u64,
    owner: String,
    req_id: u64,
    host: String,
    is_http: bool,
}

/// In-flight HTTP GET, keyed by its DNS transaction id. `stage` 0 = resolving,
/// 1 = connecting, 2 = receiving. `handle` fills in once connected. `tls` picks
/// port 443 + a host-side TLS handshake; `redirects` counts `3xx` hops followed so
/// far (capped at `MAX_REDIRECTS`).
#[table(ephemeral)]
pub struct HttpJob {
    #[primary_key]
    txid: u64,
    owner: String,
    req_id: u64,
    host: String,
    path: String,
    handle: u64,
    stage: u32,
    buffer: Vec<u8>,
    tls: bool,
    redirects: u32,
}

// ── Public result tables (apps subscribe, filter by `owner`) ────────────────────

#[table(public)]
#[derive(Debug)]
pub struct Connection {
    #[primary_key(auto_inc)]
    pub id: u64,
    pub owner: String,
    pub req_id: u64,
    pub handle: u64,
    pub status: String,
    pub error: String,
}

#[table(public)]
#[derive(Debug)]
pub struct Inbound {
    #[primary_key(auto_inc)]
    pub id: u64,
    pub owner: String,
    pub handle: u64,
    pub data: Vec<u8>,
}

#[table(public)]
#[derive(Debug)]
pub struct ConnClosed {
    #[primary_key(auto_inc)]
    pub id: u64,
    pub owner: String,
    pub handle: u64,
}

#[table(public)]
#[derive(Debug)]
pub struct Resolved {
    #[primary_key(auto_inc)]
    pub id: u64,
    pub owner: String,
    pub req_id: u64,
    pub host: String,
    pub ip: String,
    pub error: String,
}

#[table(public)]
#[derive(Debug)]
pub struct HttpResponse {
    #[primary_key(auto_inc)]
    pub id: u64,
    pub owner: String,
    pub req_id: u64,
    pub status_line: String,
    pub body: Vec<u8>,
    pub error: String,
    pub done: bool,
    /// The URL the response actually came from after following any redirects.
    /// Equals the requested URL when there were none. Apps should treat this as
    /// the document's base when resolving relative sub-resources/links, otherwise
    /// a redirected page's relative URLs resolve against the wrong (original) host.
    pub final_host: String,
    pub final_path: String,
    pub final_tls: bool,
}

// ── Lifecycle ───────────────────────────────────────────────────────────────────

#[reducer(on = "load")]
fn on_load<Caps>(ctx: ReducerContext<Caps>)
where
    Caps: CanInsert<NetCfg>,
{
    let dns_handle = match udp_bind("0.0.0.0".to_string(), 0) {
        Ok(h) => h,
        Err(err) => {
            ctx.log(&format!("network: failed to bind DNS UDP socket: {err}"));
            0
        }
    };
    let _ = ctx.current.tables.netcfg().insert(NetCfg {
        id: 0,
        dns_handle,
        next_txid: 1,
    });
    ctx.log("network: broker ready (TCP + DNS + HTTP)");
}

// ── Raw TCP broker ──────────────────────────────────────────────────────────────

#[reducer]
fn connect<Caps>(ctx: ReducerContext<Caps>, req_id: u64, ip: String, port: u32)
where
    Caps: CanInsert<TcpPending>,
{
    let owner = ctx.caller_module_name.clone();
    match tcp_connect(ip, port as u16) {
        Ok(handle) => {
            let _ = ctx.current.tables.tcppending().insert(TcpPending {
                handle,
                owner,
                req_id,
                kind: 0,
                job_txid: 0,
            });
        }
        Err(err) => ctx.log(&format!("network: connect failed for {owner}: {err}")),
    }
}

#[reducer]
fn send<Caps>(ctx: ReducerContext<Caps>, handle: u64, data: Vec<u8>)
where
    Caps: CanRead<TcpPending>,
{
    match ctx.current.tables.tcppending().get(handle) {
        Some(p) if p.owner == ctx.caller_module_name => {
            if let Err(err) = tcp_send(handle, data) {
                ctx.log(&format!("network: send failed on {handle}: {err}"));
            }
        }
        Some(_) => ctx.log(&format!(
            "network: '{}' tried to send on handle {handle} it does not own",
            ctx.caller_module_name
        )),
        None => ctx.log(&format!("network: send on unknown handle {handle}")),
    }
}

#[reducer]
fn close<Caps>(ctx: ReducerContext<Caps>, handle: u64)
where
    Caps: CanRead<TcpPending> + CanDelete<TcpPending>,
{
    match ctx.current.tables.tcppending().get(handle) {
        Some(p) if p.owner == ctx.caller_module_name => {
            let _ = tcp_close(handle);
            let _ = ctx.current.tables.tcppending().delete(handle);
        }
        Some(_) => ctx.log(&format!(
            "network: '{}' tried to close handle {handle} it does not own",
            ctx.caller_module_name
        )),
        None => {}
    }
}

// ── DNS resolver ────────────────────────────────────────────────────────────────

#[reducer]
fn resolve<Caps>(ctx: ReducerContext<Caps>, req_id: u64, host: String)
where
    Caps: CanRead<NetCfg> + CanUpdate<NetCfg> + CanInsert<DnsPending>,
{
    let owner = ctx.caller_module_name.clone();
    let Some(txid) = send_dns_query(&ctx, &host) else {
        return;
    };
    let _ = ctx.current.tables.dnspending().insert(DnsPending {
        txid: txid as u64,
        owner,
        req_id,
        host,
        is_http: false,
    });
}

// ── HTTP GET helper ─────────────────────────────────────────────────────────────

#[reducer]
fn http_get<Caps>(ctx: ReducerContext<Caps>, req_id: u64, host: String, path: String, tls: bool)
where
    Caps: CanRead<NetCfg> + CanUpdate<NetCfg> + CanInsert<DnsPending> + CanInsert<HttpJob>,
{
    let owner = ctx.caller_module_name.clone();
    start_http_job(&ctx, owner, req_id, host, path, tls, 0);
}

/// Kick off (or continue, after a redirect) an HTTP job: fire the DNS query and
/// record the pending DNS + HTTP rows keyed by its txid. `redirects` carries the
/// hop count forward so a redirect chain stays bounded.
fn start_http_job<Caps>(
    ctx: &ReducerContext<Caps>,
    owner: String,
    req_id: u64,
    host: String,
    path: String,
    tls: bool,
    redirects: u32,
) where
    Caps: CanRead<NetCfg> + CanUpdate<NetCfg> + CanInsert<DnsPending> + CanInsert<HttpJob>,
{
    let Some(txid) = send_dns_query(ctx, &host) else {
        return;
    };
    let _ = ctx.current.tables.dnspending().insert(DnsPending {
        txid: txid as u64,
        owner: owner.clone(),
        req_id,
        host: host.clone(),
        is_http: true,
    });
    let _ = ctx.current.tables.httpjob().insert(HttpJob {
        txid: txid as u64,
        owner,
        req_id,
        host,
        path,
        handle: 0,
        stage: 0,
        buffer: Vec::new(),
        tls,
        redirects,
    });
}

// ── Authority event pump ────────────────────────────────────────────────────────

#[reducer(on = "network")]
fn on_network<Caps>(ctx: ReducerContext<Caps>, event: NetworkEvent)
where
    Caps: CanRead<TcpPending>
        + CanDelete<TcpPending>
        + CanRead<DnsPending>
        + CanInsert<TcpPending>
        + CanInsert<DnsPending>
        + CanDelete<DnsPending>
        + CanRead<NetCfg>
        + CanUpdate<NetCfg>
        + CanRead<HttpJob>
        + CanUpdate<HttpJob>
        + CanInsert<HttpJob>
        + CanDelete<HttpJob>
        + CanInsert<Connection>
        + CanInsert<Inbound>
        + CanInsert<ConnClosed>
        + CanInsert<Resolved>
        + CanInsert<HttpResponse>,
{
    match event {
        NetworkEvent::Connected { handle } => {
            let Some(p) = ctx.current.tables.tcppending().get(handle) else {
                return;
            };
            if p.kind == 0 {
                let _ = ctx.current.tables.connection().insert(Connection {
                    id: 0,
                    owner: p.owner,
                    req_id: p.req_id,
                    handle,
                    status: "connected".to_string(),
                    error: String::new(),
                });
            } else if let Some(mut job) = ctx.current.tables.httpjob().get(p.job_txid) {
                let request = format!(
                    "GET {} HTTP/1.1\r\n\
                     Host: {}\r\n\
                     User-Agent: Interstice-Browser/0.1\r\n\
                     Accept: text/html,*/*\r\n\
                     Accept-Encoding: gzip, deflate\r\n\
                     Connection: close\r\n\r\n",
                    job.path, job.host
                );
                if let Err(err) = tcp_send(handle, request.into_bytes()) {
                    finish_http_error(&ctx, &job, &format!("send failed: {err}"));
                    let _ = ctx.current.tables.httpjob().delete(job.txid);
                    let _ = ctx.current.tables.tcppending().delete(handle);
                } else {
                    job.stage = 2;
                    let _ = ctx.current.tables.httpjob().update(job);
                }
            }
        }

        NetworkEvent::ConnectFailed { handle, error } | NetworkEvent::Failed { handle, error } => {
            let Some(p) = ctx.current.tables.tcppending().get(handle) else {
                return;
            };
            if p.kind == 0 {
                let _ = ctx.current.tables.connection().insert(Connection {
                    id: 0,
                    owner: p.owner,
                    req_id: p.req_id,
                    handle,
                    status: "failed".to_string(),
                    error,
                });
            } else if let Some(job) = ctx.current.tables.httpjob().get(p.job_txid) {
                finish_http_error(&ctx, &job, &error);
                let _ = ctx.current.tables.httpjob().delete(job.txid);
            }
            let _ = ctx.current.tables.tcppending().delete(handle);
        }

        NetworkEvent::Received { handle, data } => {
            let Some(p) = ctx.current.tables.tcppending().get(handle) else {
                return;
            };
            if p.kind == 0 {
                let _ = ctx.current.tables.inbound().insert(Inbound {
                    id: 0,
                    owner: p.owner,
                    handle,
                    data,
                });
            } else if let Some(mut job) = ctx.current.tables.httpjob().get(p.job_txid) {
                job.buffer.extend_from_slice(&data);
                let _ = ctx.current.tables.httpjob().update(job);
            }
        }

        NetworkEvent::Closed { handle } => {
            let Some(p) = ctx.current.tables.tcppending().get(handle) else {
                return;
            };
            if p.kind == 0 {
                let _ = ctx.current.tables.connclosed().insert(ConnClosed {
                    id: 0,
                    owner: p.owner,
                    handle,
                });
            } else if let Some(job) = ctx.current.tables.httpjob().get(p.job_txid) {
                let _ = ctx.current.tables.httpjob().delete(job.txid);
                let parsed = parse_http(&job.buffer);

                // Follow a 3xx redirect (but not 304 Not Modified) by re-issuing the
                // request at its Location, reusing the caller's req_id so the app sees
                // one logical response. Bounded by MAX_REDIRECTS.
                if (300..400).contains(&parsed.status)
                    && parsed.status != 304
                    && job.redirects < MAX_REDIRECTS
                {
                    if let Some(location) = header(&parsed.headers, "location") {
                        if let Some((host, path, tls)) =
                            parse_redirect(&job.host, &job.path, job.tls, location)
                        {
                            start_http_job(
                                &ctx,
                                job.owner.clone(),
                                job.req_id,
                                host,
                                path,
                                tls,
                                job.redirects + 1,
                            );
                            let _ = ctx.current.tables.tcppending().delete(handle);
                            return;
                        }
                    }
                }

                let body = decode_body(&parsed);
                let _ = ctx.current.tables.httpresponse().insert(HttpResponse {
                    id: 0,
                    owner: job.owner,
                    req_id: job.req_id,
                    status_line: parsed.status_line,
                    body,
                    error: String::new(),
                    done: true,
                    // `job` already reflects the final hop: each redirect re-issues a
                    // fresh job carrying the Location's host/path/tls, so these are the
                    // URL the body actually came from.
                    final_host: job.host,
                    final_path: job.path,
                    final_tls: job.tls,
                });
            }
            let _ = ctx.current.tables.tcppending().delete(handle);
        }

        NetworkEvent::UdpReceived { data, .. } => {
            let Some((txid, ip_opt)) = dns::parse_response(&data) else {
                return;
            };
            let Some(pending) = ctx.current.tables.dnspending().get(txid as u64) else {
                return;
            };
            let _ = ctx.current.tables.dnspending().delete(txid as u64);

            if !pending.is_http {
                let (ip, error) = match &ip_opt {
                    Some(ip) => (ip.clone(), String::new()),
                    None => (String::new(), "no A record".to_string()),
                };
                let _ = ctx.current.tables.resolved().insert(Resolved {
                    id: 0,
                    owner: pending.owner,
                    req_id: pending.req_id,
                    host: pending.host,
                    ip,
                    error,
                });
                return;
            }

            // HTTP job: turn the resolved IP into a TCP connection, or fail it.
            let Some(mut job) = ctx.current.tables.httpjob().get(txid as u64) else {
                return;
            };
            match ip_opt {
                Some(ip) => {
                    // For https, terminate TLS host-side: connect on 443 and pass the
                    // hostname for SNI + certificate validation (we resolved an IP, but
                    // the cert is issued to the name).
                    let connect = if job.tls {
                        tcp_connect_tls(ip, HTTPS_PORT, job.host.clone())
                    } else {
                        tcp_connect(ip, HTTP_PORT)
                    };
                    match connect {
                    Ok(handle) => {
                        job.handle = handle;
                        job.stage = 1;
                        let owner = job.owner.clone();
                        let req_id = job.req_id;
                        let _ = ctx.current.tables.httpjob().update(job);
                        let _ = ctx.current.tables.tcppending().insert(TcpPending {
                            handle,
                            owner,
                            req_id,
                            kind: 1,
                            job_txid: txid as u64,
                        });
                    }
                    Err(err) => {
                        finish_http_error(&ctx, &job, &format!("connect failed: {err}"));
                        let _ = ctx.current.tables.httpjob().delete(txid as u64);
                    }
                    }
                }
                None => {
                    finish_http_error(&ctx, &job, "DNS resolution failed");
                    let _ = ctx.current.tables.httpjob().delete(txid as u64);
                }
            }
        }

        NetworkEvent::Accepted { .. } => {
            // The broker does not expose listening sockets yet.
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────────

/// Allocate a transaction id, send a DNS A-record query for `host`, and return
/// the txid. `None` if the DNS socket isn't ready.
fn send_dns_query<Caps>(ctx: &ReducerContext<Caps>, host: &str) -> Option<u16>
where
    Caps: CanRead<NetCfg> + CanUpdate<NetCfg>,
{
    let mut cfg = ctx.current.tables.netcfg().get(0)?;
    let txid = (cfg.next_txid & 0xFFFF) as u16;
    cfg.next_txid = cfg.next_txid.wrapping_add(1);
    let dns_handle = cfg.dns_handle;
    let _ = ctx.current.tables.netcfg().update(cfg);

    let query = dns::build_query(txid, host);
    if let Err(err) = udp_send_to(dns_handle, DNS_SERVER.to_string(), DNS_PORT, query) {
        ctx.log(&format!("network: DNS query for {host} failed: {err}"));
        return None;
    }
    Some(txid)
}

/// Emit a terminal `HttpResponse` carrying an error for `job`.
fn finish_http_error<Caps>(ctx: &ReducerContext<Caps>, job: &HttpJob, error: &str)
where
    Caps: CanInsert<HttpResponse>,
{
    let _ = ctx.current.tables.httpresponse().insert(HttpResponse {
        id: 0,
        owner: job.owner.clone(),
        req_id: job.req_id,
        status_line: String::new(),
        body: Vec::new(),
        error: error.to_string(),
        done: true,
        final_host: job.host.clone(),
        final_path: job.path.clone(),
        final_tls: job.tls,
    });
}

/// A raw HTTP/1.1 response split into its parts. Headers keep their original
/// casing in `headers` but are matched case-insensitively via [`header`]. `body`
/// is the still-encoded payload (possibly chunked and/or gzip/deflate).
struct ParsedResponse {
    status_line: String,
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

/// Parse a raw HTTP/1.1 response into status line, status code, headers, and the
/// (still-encoded) body. Operates on raw bytes so binary bodies survive intact.
/// Tolerates a missing body or header/body separator.
fn parse_http(raw: &[u8]) -> ParsedResponse {
    let (head, body) = match find_subslice(raw, b"\r\n\r\n") {
        Some(idx) => (&raw[..idx], raw[idx + 4..].to_vec()),
        None => (raw, Vec::new()),
    };
    let head = String::from_utf8_lossy(head);
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or("").to_string();
    // Status line: "HTTP/1.1 301 Moved Permanently" → 301.
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or(0);
    let headers = lines
        .filter_map(|line| {
            line.split_once(':')
                .map(|(name, value)| (name.trim().to_string(), value.trim().to_string()))
        })
        .collect();
    ParsedResponse {
        status_line,
        status,
        headers,
        body,
    }
}

/// Case-insensitive lookup of the first header named `name`.
fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// Decode a response body per its headers: undo `Transfer-Encoding: chunked`, then
/// `Content-Encoding: gzip`/`deflate`. Anything unrecognised is returned as-is so a
/// surprising encoding degrades to garbled-but-present rather than empty.
fn decode_body(parsed: &ParsedResponse) -> Vec<u8> {
    let mut body = parsed.body.clone();
    if header(&parsed.headers, "transfer-encoding")
        .map(|v| v.to_ascii_lowercase().contains("chunked"))
        .unwrap_or(false)
    {
        body = dechunk(&body);
    }
    match header(&parsed.headers, "content-encoding").map(|v| v.to_ascii_lowercase()) {
        Some(enc) if enc.contains("gzip") => decompress_gzip(&body),
        Some(enc) if enc.contains("deflate") => decompress_deflate(&body),
        _ => body,
    }
}

/// Decode HTTP/1.1 chunked transfer-encoding into the raw byte stream. Tolerates a
/// truncated final chunk (returns what was decoded so far).
fn dechunk(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let Some(rel) = find_subslice(&data[i..], b"\r\n") else {
            break;
        };
        let line_end = i + rel;
        // The size line may carry chunk extensions after a ';' — ignore them.
        let size_str = String::from_utf8_lossy(&data[i..line_end]);
        let size_hex = size_str.split(';').next().unwrap_or("").trim();
        let Ok(size) = usize::from_str_radix(size_hex, 16) else {
            break;
        };
        i = line_end + 2;
        if size == 0 {
            break; // last chunk
        }
        let end = (i + size).min(data.len());
        out.extend_from_slice(&data[i..end]);
        i = end;
        // Skip the CRLF trailing each chunk's data.
        if data.len() >= i + 2 && &data[i..i + 2] == b"\r\n" {
            i += 2;
        }
    }
    out
}

/// Inflate a gzip-encoded body; on error return the input untouched.
fn decompress_gzip(data: &[u8]) -> Vec<u8> {
    use std::io::Read;
    let mut out = Vec::new();
    let mut decoder = flate2::read::GzDecoder::new(data);
    match decoder.read_to_end(&mut out) {
        Ok(_) => out,
        Err(_) => data.to_vec(),
    }
}

/// Inflate a `Content-Encoding: deflate` body. Servers disagree on whether this is
/// zlib-wrapped or raw DEFLATE, so try zlib first and fall back to raw.
fn decompress_deflate(data: &[u8]) -> Vec<u8> {
    use std::io::Read;
    let mut out = Vec::new();
    if flate2::read::ZlibDecoder::new(data)
        .read_to_end(&mut out)
        .is_ok()
    {
        return out;
    }
    out.clear();
    match flate2::read::DeflateDecoder::new(data).read_to_end(&mut out) {
        Ok(_) => out,
        Err(_) => data.to_vec(),
    }
}

/// Resolve a `Location` header against the request that produced it, yielding the
/// next `(host, path, tls)`. Handles absolute (`http`/`https`), protocol-relative
/// (`//host`), root-relative (`/path`), and document-relative redirects.
fn parse_redirect(
    cur_host: &str,
    cur_path: &str,
    cur_tls: bool,
    location: &str,
) -> Option<(String, String, bool)> {
    let loc = location.split('#').next().unwrap_or(location).trim();
    if loc.is_empty() {
        return None;
    }
    if let Some(rest) = loc.strip_prefix("https://") {
        let (host, path) = split_host_path(rest);
        return Some((host, path, true));
    }
    if let Some(rest) = loc.strip_prefix("http://") {
        let (host, path) = split_host_path(rest);
        return Some((host, path, false));
    }
    if let Some(rest) = loc.strip_prefix("//") {
        let (host, path) = split_host_path(rest);
        return Some((host, path, cur_tls));
    }
    if loc.starts_with('/') {
        return Some((cur_host.to_string(), loc.to_string(), cur_tls));
    }
    // Document-relative: resolve against the current path's directory.
    let dir = match cur_path.rfind('/') {
        Some(i) => &cur_path[..=i],
        None => "/",
    };
    Some((cur_host.to_string(), format!("{dir}{loc}"), cur_tls))
}

/// Split `host[:port]/path` (no scheme) into `(host, path)`, dropping the port and
/// defaulting an absent path to `/`.
fn split_host_path(s: &str) -> (String, String) {
    match s.find('/') {
        Some(i) => {
            let host = s[..i].split(':').next().unwrap_or(&s[..i]).to_string();
            (host, s[i..].to_string())
        }
        None => {
            let host = s.split(':').next().unwrap_or(s).to_string();
            (host, "/".to_string())
        }
    }
}

/// Index of the first occurrence of `needle` in `haystack`.
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
}
