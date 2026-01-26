use crate::Node;
use interstice_abi::host::AbortRequest;

impl Node {
    pub(crate) fn handle_abort(&self, abort_request: AbortRequest) {
        println!("[ABORT] {}", abort_request.message);
    }
}
