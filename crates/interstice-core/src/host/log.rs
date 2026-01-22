use interstice_abi::host::LogRequest;

use crate::runtime::Runtime;

impl Runtime {
    pub(crate) fn handle_log(&self, caller_module_name: String, log_request: LogRequest) {
        println!("[{}] {}", caller_module_name, log_request.message);
    }
}
