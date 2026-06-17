pub mod instance;
pub mod linker;

use interstice_abi::ModuleSchema;
use std::sync::Arc;

use crate::runtime::Runtime;

pub struct StoreState {
    pub runtime: Arc<Runtime>,
    pub module_schema: Arc<ModuleSchema>,
    /// Encoded `insert_row` response stashed when it overflowed the wasm's
    /// fast-path response buffer.  The SDK immediately retries the call with a
    /// larger buffer; on that retry the host returns these bytes WITHOUT
    /// re-running the side-effecting insert, so the transaction is applied
    /// exactly once.  See `linker.rs` `interstice_insert_row`.
    pub pending_insert_response: Option<Vec<u8>>,
}

impl StoreState {
    pub fn new(runtime: Arc<Runtime>) -> Self {
        Self {
            runtime,
            module_schema: Arc::new(ModuleSchema::empty()),
            pending_insert_response: None,
        }
    }
}

