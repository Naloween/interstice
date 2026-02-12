use crate::error::IntersticeError;
use crate::node::NodeId;
use crate::runtime::reducer::CallFrameKind;
use interstice_abi::encode;
use serde::Serialize;

const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

fn fnv1a(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

pub(crate) fn seed_from_call<T: Serialize>(
    caller_node_id: &NodeId,
    module_name: &str,
    entry_name: &str,
    kind: CallFrameKind,
    call_sequence: u64,
    args: &T,
) -> Result<u64, IntersticeError> {
    let args_bytes = encode(args).map_err(|err| {
        IntersticeError::Internal(format!(
            "failed to serialize deterministic random seed: {err}"
        ))
    })?;

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
    hash = fnv1a(hash, &call_sequence.to_le_bytes());
    hash = fnv1a(hash, &args_bytes);

    if hash == 0 {
        hash = FNV_OFFSET;
    }

    Ok(hash)
}

pub(crate) fn next_u64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}
