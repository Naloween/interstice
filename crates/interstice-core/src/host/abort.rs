use interstice_abi::host::AbortRequest;

use crate::runtime::Runtime;

impl Runtime {
    pub fn handle_abort(&self, abort_request: AbortRequest) {
        println!("[ABORT] {}", abort_request.message);
    }
}
