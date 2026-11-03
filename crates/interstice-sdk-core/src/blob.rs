//! [`Blob`]: a handle to an immutable host-side byte buffer, for passing bulk
//! data (textures, audio, file contents) to another module without encoding
//! it into the call arguments.
//!
//! ```ignore
//! let pixels: Vec<u8> = render();
//! graphics.reducers.update_texture(id, 0, 0, w, h, Blob::new(&pixels))?;
//! ```
//!
//! A blob lives as long as the call that created it and the reducer calls it
//! issued (they run after it commits, and still see it); queries it calls can
//! read it too. Don't keep a `Blob` in an instance cache. Blobs cannot be stored in tables.
//! A blob crossing to another node travels as bytes and is re-registered on
//! arrival, so a `Blob` parameter works the same for remote callers.

use interstice_abi::{IntersticeAbiError, IntersticeValue};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Blob {
    id: u64,
}

impl Blob {
    /// Copy `bytes` into a new blob owned by the current call.
    ///
    /// Panics outside a reducer/query call.
    pub fn new(bytes: &[u8]) -> Self {
        let id = unsafe {
            crate::host_calls::interstice_blob_create(bytes.as_ptr() as i32, bytes.len() as i32)
        };
        assert!(id >= 0, "Blob::new: no active call to own the blob");
        Self { id: id as u64 }
    }

    /// Host-side id; meaningful only within the current call tree.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Size in bytes. Panics if the blob is no longer alive.
    pub fn len(&self) -> usize {
        let len = unsafe { crate::host_calls::interstice_blob_len(self.id as i64) };
        assert!(len >= 0, "blob #{} is not alive in this call", self.id);
        len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Copy bytes from `offset` into `buf`; returns how many were copied.
    pub fn read_into(&self, offset: usize, buf: &mut [u8]) -> usize {
        let n = unsafe {
            crate::host_calls::interstice_blob_read(
                self.id as i64,
                offset as i64,
                buf.as_mut_ptr() as i32,
                buf.len() as i32,
            )
        };
        assert!(n >= 0, "blob #{} is not alive in this call", self.id);
        n as usize
    }

    /// Copy the whole blob into guest memory.
    pub fn to_vec(&self) -> Vec<u8> {
        let mut out = vec![0u8; self.len()];
        let n = self.read_into(0, &mut out);
        out.truncate(n);
        out
    }
}

impl From<Blob> for IntersticeValue {
    fn from(blob: Blob) -> Self {
        IntersticeValue::Blob(blob.id)
    }
}

impl TryFrom<IntersticeValue> for Blob {
    type Error = IntersticeAbiError;

    /// Accepts a handle, or inline bytes (a call from another node), which
    /// are registered as a new blob of the current call.
    fn try_from(value: IntersticeValue) -> Result<Self, Self::Error> {
        match value {
            IntersticeValue::Blob(id) => Ok(Blob { id }),
            IntersticeValue::Bytes(bytes) => Ok(Blob::new(&bytes)),
            other => Err(IntersticeAbiError::ConversionError(format!(
                "Expected IntersticeValue::Blob, got {other}"
            ))),
        }
    }
}
