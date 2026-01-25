pub mod host_calls;
pub mod registry;

use crate::host_calls::log;
use interstice_abi::ReducerContext;

pub trait HostLog {
    fn log(&self, message: &str);
}

impl HostLog for ReducerContext {
    fn log(&self, message: &str) {
        log(message);
    }
}
