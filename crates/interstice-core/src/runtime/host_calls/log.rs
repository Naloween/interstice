use crate::runtime::Runtime;
use interstice_abi::LogRequest;

impl Runtime {
    pub(crate) fn handle_log(&self, caller_module_name: String, log_request: LogRequest) {
        println!("[{}] {}", caller_module_name, log_request.message);
    }
}
