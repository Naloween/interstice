use crate::node::NodeId;
use crate::runtime::reducer::CallFrameKind;

pub(crate) const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

pub(crate) fn fnv1a(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

pub(crate) fn seed_from_call(
    caller_node_id: &NodeId,
    module_name: &str,
    entry_name: &str,
    kind: CallFrameKind,
    tx_id: u64,
) -> u64 {
    let mut hash = FNV_OFFSET;
    hash = fnv1a(hash, caller_node_id.as_bytes());
    hash = fnv1a(hash, module_name.as_bytes());
    hash = fnv1a(hash, entry_name.as_bytes());
    hash = fnv1a(
        hash,
        &[match kind {
            CallFrameKind::Reducer => 1,
            CallFrameKind::Query => 2,
        }],
    );
    hash = fnv1a(hash, &tx_id.to_le_bytes());

    if hash == 0 {
        hash = FNV_OFFSET;
    }

    hash
}

/// The inputs a transaction gets from the engine (DESIGN §6.5). Every frame
/// of the transaction uses the root frame's copy.
#[derive(Debug, Clone, Copy)]
pub struct Tx {
    pub id: u64,
    pub timestamp_ms: u64,
    /// SplitMix64 state, seeded from the transaction identity.
    pub rng_state: u64,
    /// Blobs created so far by the root frame, and by nested frames.
    pub blobs: u32,
    pub nested_blobs: u32,
}

/// Blob ids made by nested frames (queries the transaction called) have
/// this bit set, so the root frame's ids don't depend on them.
const NESTED_BLOB_BIT: u64 = 1 << 23;

impl Tx {
    pub fn new(id: u64, timestamp_ms: u64, rng_state: u64) -> Self {
        Self { id, timestamp_ms, rng_state, blobs: 0, nested_blobs: 0 }
    }

    /// The next blob id of this transaction: `(id << 24) | n`.
    pub fn next_blob_id(&mut self, nested: bool) -> u64 {
        let n = if nested {
            self.nested_blobs += 1;
            NESTED_BLOB_BIT | self.nested_blobs as u64
        } else {
            self.blobs += 1;
            self.blobs as u64
        };
        (self.id << 24) | n
    }
}

/// Hands out transaction ids and timestamps together, under one lock, so a
/// timestamp never decreases as ids grow: a cut of a log by time is also a
/// cut by id (DESIGN §6.5, §6.7).
#[derive(Debug)]
pub(crate) struct TxClock {
    /// `(next id, last timestamp)`. Ids start at 1: `0` means "no commit".
    state: parking_lot::Mutex<(u64, u64)>,
}

impl TxClock {
    pub fn new() -> Self {
        Self { state: parking_lot::Mutex::new((1, 0)) }
    }

    /// A new `(id, timestamp_ms)`: the wall clock, clamped so it never goes
    /// below an earlier one.
    pub fn next(&self, now_ms: u64) -> (u64, u64) {
        let mut state = self.state.lock();
        let id = state.0;
        state.0 += 1;
        state.1 = state.1.max(now_ms);
        (id, state.1)
    }

    /// Continue above a restored log's largest id and timestamp.
    pub fn advance_past(&self, (id, timestamp_ms): (u64, u64)) {
        let mut state = self.state.lock();
        state.0 = state.0.max(id + 1);
        state.1 = state.1.max(timestamp_ms);
    }
}

pub(crate) fn wall_clock_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) fn next_u64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transaction_clock_never_goes_back() {
        let clock = TxClock::new();
        assert_eq!(clock.next(100), (1, 100));
        assert_eq!(clock.next(90), (2, 100));
        assert_eq!(clock.next(120), (3, 120));
        clock.advance_past((10, 500));
        assert_eq!(clock.next(130), (11, 500));
        clock.advance_past((4, 0));
        assert_eq!(clock.next(600), (12, 600));
    }
}
