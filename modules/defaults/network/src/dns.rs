//! Minimal DNS-over-UDP A-record codec. Hand-rolled so the module keeps a single
//! dependency (`interstice-sdk`) — no external crates. Only what the broker needs:
//! build a query for one hostname, and pull the first IPv4 answer out of the reply.

/// Build a standard recursive A-record query for `host` with transaction id `txid`.
pub fn build_query(txid: u16, host: &str) -> Vec<u8> {
    let mut p = Vec::with_capacity(host.len() + 18);
    p.extend_from_slice(&txid.to_be_bytes());
    p.extend_from_slice(&0x0100u16.to_be_bytes()); // flags: RD (recursion desired)
    p.extend_from_slice(&1u16.to_be_bytes()); // qdcount
    p.extend_from_slice(&0u16.to_be_bytes()); // ancount
    p.extend_from_slice(&0u16.to_be_bytes()); // nscount
    p.extend_from_slice(&0u16.to_be_bytes()); // arcount
    for label in host.split('.').filter(|l| !l.is_empty()) {
        p.push(label.len() as u8);
        p.extend_from_slice(label.as_bytes());
    }
    p.push(0); // root label terminates QNAME
    p.extend_from_slice(&1u16.to_be_bytes()); // QTYPE = A
    p.extend_from_slice(&1u16.to_be_bytes()); // QCLASS = IN
    p
}

/// Parse a reply. Returns `(txid, Some("a.b.c.d"))` for the first A record, or
/// `(txid, None)` when the response has no usable IPv4 answer. `None` overall if
/// the buffer is too short to even read the transaction id.
pub fn parse_response(buf: &[u8]) -> Option<(u16, Option<String>)> {
    if buf.len() < 12 {
        return None;
    }
    let txid = u16::from_be_bytes([buf[0], buf[1]]);
    let qd = u16::from_be_bytes([buf[4], buf[5]]);
    let an = u16::from_be_bytes([buf[6], buf[7]]);

    let mut pos = 12;
    for _ in 0..qd {
        pos = skip_name(buf, pos)?;
        pos += 4; // QTYPE + QCLASS
    }
    for _ in 0..an {
        pos = skip_name(buf, pos)?;
        if pos + 10 > buf.len() {
            return Some((txid, None));
        }
        let rtype = u16::from_be_bytes([buf[pos], buf[pos + 1]]);
        let rdlen = u16::from_be_bytes([buf[pos + 8], buf[pos + 9]]) as usize;
        pos += 10;
        if rtype == 1 && rdlen == 4 && pos + 4 <= buf.len() {
            return Some((
                txid,
                Some(format!(
                    "{}.{}.{}.{}",
                    buf[pos],
                    buf[pos + 1],
                    buf[pos + 2],
                    buf[pos + 3]
                )),
            ));
        }
        pos += rdlen;
    }
    Some((txid, None))
}

/// Advance past a DNS name starting at `pos`, returning the index just after it.
/// A 0xC0 compression pointer terminates the name (2 bytes).
fn skip_name(buf: &[u8], mut pos: usize) -> Option<usize> {
    loop {
        if pos >= buf.len() {
            return None;
        }
        let len = buf[pos];
        if len == 0 {
            return Some(pos + 1);
        }
        if len & 0xC0 == 0xC0 {
            return Some(pos + 2);
        }
        pos += 1 + len as usize;
    }
}
