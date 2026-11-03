use crate::node::NodeId;
use crate::runtime::reducer::CallFrameKind;

const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

fn fnv1a(mut hash: u64, bytes: &[u8]) -> u64 {
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
}

/// Wall-clock ms, clamped so it never goes below `last` (which it raises).
pub(crate) fn monotonic_ms(last: &std::sync::atomic::AtomicU64, now_ms: u64) -> u64 {
    let prev = last.fetch_max(now_ms, std::sync::atomic::Ordering::AcqRel);
    prev.max(now_ms)
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
    use std::sync::atomic::AtomicU64;

    #[test]
    fn transaction_clock_never_goes_back() {
        let last = AtomicU64::new(0);
        assert_eq!(monotonic_ms(&last, 100), 100);
        assert_eq!(monotonic_ms(&last, 90), 100);
        assert_eq!(monotonic_ms(&last, 120), 120);
    }
}
