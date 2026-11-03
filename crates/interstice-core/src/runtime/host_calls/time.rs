use crate::runtime::Runtime;
use crate::runtime::reducer::CALL_STACK;
use interstice_abi::{TimeRequest, TimeResponse};

impl Runtime {
    /// The transaction's timestamp: constant for every call in it (DESIGN §6.5).
    pub(crate) fn handle_time(&self, _request: TimeRequest) -> TimeResponse {
        match CALL_STACK.with(|s| s.borrow().first().map(|f| f.tx.timestamp_ms)) {
            Some(unix_ms) => TimeResponse::Ok { unix_ms },
            None => TimeResponse::Err("Time call outside of an active transaction".into()),
        }
    }
}
