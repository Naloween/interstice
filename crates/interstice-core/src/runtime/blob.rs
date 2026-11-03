//! Blobs: immutable host-side byte buffers for moving bulk data between
//! modules (textures, audio, file contents) without an encode/decode per hop.
//!
//! A blob lives in a [`BlobScope`] (DESIGN §6.6). Every reducer frame owns a
//! scope whose parent is the scope its job inherited: none for a top-level
//! job, the caller's root scope for a callee in a call batch. Query frames
//! share their caller's scope. An id resolves through the current frame's
//! scope chain, so a callee sees everything its callers hold. A scope is
//! freed when the last frame or batch referencing it is gone. Blobs never
//! enter table rows. Across nodes they travel inline (see [`inline_blobs`]);
//! the receiving guest's `Blob` conversion re-registers inline bytes as a
//! fresh blob.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use interstice_abi::IntersticeValue;
use parking_lot::Mutex;

use crate::runtime::reducer::CALL_STACK;

static NEXT_BLOB_ID: AtomicU64 = AtomicU64::new(1);

pub type BlobData = Arc<[u8]>;

#[derive(Debug, Default)]
pub struct BlobScope {
    blobs: Mutex<HashMap<u64, BlobData>>,
    parent: Option<Arc<BlobScope>>,
}

impl BlobScope {
    pub fn new(parent: Option<Arc<BlobScope>>) -> Arc<Self> {
        Arc::new(Self {
            blobs: Mutex::new(HashMap::new()),
            parent,
        })
    }

    fn insert(&self, data: BlobData) -> u64 {
        let id = NEXT_BLOB_ID.fetch_add(1, Ordering::Relaxed);
        self.blobs.lock().insert(id, data);
        id
    }

    fn resolve(&self, id: u64) -> Option<BlobData> {
        let mut scope = Some(self);
        while let Some(s) = scope {
            if let Some(data) = s.blobs.lock().get(&id) {
                return Some(data.clone());
            }
            scope = s.parent.as_deref();
        }
        None
    }
}

/// The scope of the innermost frame on this thread, if any.
pub fn current_scope() -> Option<Arc<BlobScope>> {
    CALL_STACK.with(|s| s.borrow().last().map(|f| f.scope.clone()))
}

/// Register `data` in the current frame's scope. `None` outside any call.
pub fn create_blob(data: BlobData) -> Option<u64> {
    current_scope().map(|scope| scope.insert(data))
}

/// Look `id` up in the current frame's scope chain.
pub fn resolve_blob(id: u64) -> Option<BlobData> {
    current_scope()?.resolve(id)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_through_parent_chain_only() {
        let root = BlobScope::new(None);
        let a = root.insert(Arc::from(&b"root"[..]));
        let child = BlobScope::new(Some(root.clone()));
        let b = child.insert(Arc::from(&b"child"[..]));
        assert_eq!(&*child.resolve(a).unwrap(), b"root");
        assert_eq!(&*child.resolve(b).unwrap(), b"child");
        assert!(root.resolve(b).is_none());
    }
}
