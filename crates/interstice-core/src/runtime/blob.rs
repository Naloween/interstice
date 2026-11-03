//! Blobs: immutable host-side byte buffers for moving bulk data between
//! modules (textures, audio, file contents) without an encode/decode per hop.
//!
//! A blob belongs to the call frame that created it (or received it as inline
//! bytes) and is dropped when that frame pops. Nested calls run synchronously
//! on the caller's thread, so a callee resolves an id by walking the
//! thread's [`CALL_STACK`]: anything its callers hold is visible, nothing
//! outlives the outermost call, and nothing leaks. Blobs never enter table
//! rows. Across nodes they travel inline (see [`inline_blobs`]); the receiving
//! guest's `Blob` conversion re-registers inline bytes as a fresh blob.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use interstice_abi::IntersticeValue;

use crate::runtime::reducer::CALL_STACK;

static NEXT_BLOB_ID: AtomicU64 = AtomicU64::new(1);

pub type BlobData = Arc<[u8]>;

/// Register `data` in the innermost call frame. `None` outside any call.
pub fn create_blob(data: BlobData) -> Option<u64> {
    CALL_STACK.with(|s| {
        let mut stack = s.borrow_mut();
        let frame = stack.last_mut()?;
        let id = NEXT_BLOB_ID.fetch_add(1, Ordering::Relaxed);
        frame.blobs.insert(id, data);
        Some(id)
    })
}

/// Look `id` up in the current call or any of its callers.
pub fn resolve_blob(id: u64) -> Option<BlobData> {
    CALL_STACK.with(|s| {
        s.borrow()
            .iter()
            .rev()
            .find_map(|frame| frame.blobs.get(&id).cloned())
    })
}

/// Replace every `Blob(id)` in `value` with its bytes, for sending somewhere
/// the handle means nothing (another node). Unknown ids are an error.
pub fn inline_blobs(value: &mut IntersticeValue) -> Result<(), String> {
    match value {
        IntersticeValue::Blob(id) => {
            let data = resolve_blob(*id).ok_or_else(|| format!("unknown blob #{id}"))?;
            *value = IntersticeValue::Bytes(data.to_vec());
            Ok(())
        }
        IntersticeValue::Vec(items) | IntersticeValue::Tuple(items) => {
            items.iter_mut().try_for_each(inline_blobs)
        }
        IntersticeValue::Option(Some(inner)) => inline_blobs(inner),
        IntersticeValue::Struct { fields, .. } => {
            fields.iter_mut().try_for_each(|f| inline_blobs(&mut f.value))
        }
        IntersticeValue::Enum { value, .. } => inline_blobs(value),
        _ => Ok(()),
    }
}
