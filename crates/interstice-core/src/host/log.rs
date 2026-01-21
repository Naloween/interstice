use interstice_abi::{host::LogRequest, types::ModuleId};

use crate::runtime::Runtime;

impl Runtime {
    pub fn handle_log(&self, caller: ModuleId, log_request: LogRequest) {
        println!("[{}] {}", caller, log_request.message);
    }
}
