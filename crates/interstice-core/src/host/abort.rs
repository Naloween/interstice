use interstice_abi::{decode, host_calls::AbortRequest};

use crate::runtime::Runtime;

impl Runtime {
    pub fn handle_abort(&self, bytes: &[u8]) {
        if let Ok(req) = decode::<AbortRequest>(bytes) {
            println!("[ABORT] {}", req.message);
        }
    }
}
