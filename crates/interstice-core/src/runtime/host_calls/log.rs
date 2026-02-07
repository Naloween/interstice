use crate::{
    logger::{LogLevel, LogSource},
    runtime::Runtime,
};
use interstice_abi::LogRequest;

impl Runtime {
    pub(crate) fn handle_log(&self, caller_module_name: String, log_request: LogRequest) {
        self.logger.log(
            &log_request.message,
            LogSource::Module(caller_module_name),
            LogLevel::Info,
        );
    }
}
