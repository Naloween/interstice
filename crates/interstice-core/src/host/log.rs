use interstice_abi::{decode, host_calls::LogRequest, types::ModuleId};

use crate::runtime::Runtime;

impl Runtime {
    pub fn handle_log(&self, caller: ModuleId, bytes: &[u8]) {
        if let Ok(req) = decode::<LogRequest>(bytes) {
            println!("[{}] {}", caller, req.message);
        }
    }
}
