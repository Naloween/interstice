use crate::Node;
use interstice_abi::host::LogRequest;

impl Node {
    pub(crate) fn handle_log(&self, caller_module_name: String, log_request: LogRequest) {
        println!("[{}] {}", caller_module_name, log_request.message);
    }
}
