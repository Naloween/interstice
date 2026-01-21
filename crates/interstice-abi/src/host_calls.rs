use crate::PrimitiveValue;
use serde::{Deserialize, Serialize};

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

pub const CALL_REDUCER: u32 = 1;
pub const LOG: u32 = 2;
pub const ABORT: u32 = 3;

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
