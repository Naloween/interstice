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
/// 1 = connecting, 2 = receiving. `handle` fills in once connected.
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
    buffer: String,
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
    pub body: String,
    pub error: String,
    pub done: bool,
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
fn http_get<Caps>(ctx: ReducerContext<Caps>, req_id: u64, host: String, path: String)
where
    Caps: CanRead<NetCfg> + CanUpdate<NetCfg> + CanInsert<DnsPending> + CanInsert<HttpJob>,
{
    let owner = ctx.caller_module_name.clone();
    let Some(txid) = send_dns_query(&ctx, &host) else {
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
        buffer: String::new(),
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
        + CanDelete<DnsPending>
        + CanRead<HttpJob>
        + CanUpdate<HttpJob>
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
                    "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
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
                job.buffer.push_str(&String::from_utf8_lossy(&data));
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
                let (status_line, body) = split_http(&job.buffer);
                let _ = ctx.current.tables.httpresponse().insert(HttpResponse {
                    id: 0,
                    owner: job.owner,
                    req_id: job.req_id,
                    status_line,
                    body,
                    error: String::new(),
                    done: true,
                });
                let _ = ctx.current.tables.httpjob().delete(job.txid);
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
                Some(ip) => match tcp_connect(ip, HTTP_PORT) {
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
                },
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
        body: String::new(),
        error: error.to_string(),
        done: true,
    });
}

/// Split a raw HTTP/1.1 response into (status line, body). Tolerates a missing
/// body or header/body separator.
fn split_http(raw: &str) -> (String, String) {
    let status_line = raw.lines().next().unwrap_or("").to_string();
    let body = match raw.find("\r\n\r\n") {
        Some(idx) => raw[idx + 4..].to_string(),
        None => String::new(),
    };
    (status_line, body)
}
