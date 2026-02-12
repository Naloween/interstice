use crate::runtime::Runtime;
use interstice_abi::{TimeRequest, TimeResponse};
use std::time::{SystemTime, UNIX_EPOCH};

impl Runtime {
    pub(crate) fn handle_time(&self, _request: TimeRequest) -> TimeResponse {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(duration) => TimeResponse::Ok {
                unix_ms: duration.as_millis() as u64,
            },
            Err(err) => TimeResponse::Err(err.to_string()),
        }
    }
}
