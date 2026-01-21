use crate::PrimitiveValue;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub enum HostCall {
    CallReducer(CallReducerRequest),
    Log(LogRequest),
    Abort(AbortRequest),
}

/// Host call: CALL_REDUCER
///
/// Semantics:
/// - Synchronous
/// - Stack-based
/// - Non-reentrant at reducer level
/// - Traps on error
///
/// Request: CallReducerRequest (postcard)
/// Response: PrimitiveValue (postcard)
///
/// Memory:
/// - Request buffer owned by caller
/// - Response buffer owned by host

#[derive(Debug, Serialize, Deserialize)]
pub struct CallReducerRequest {
    pub target_module: String,
    pub reducer: String,
    pub input: PrimitiveValue,
}

pub type CallReducerResponse = PrimitiveValue;

#[derive(Debug, Serialize, Deserialize)]
pub struct LogRequest {
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AbortRequest {
    pub message: String,
}
