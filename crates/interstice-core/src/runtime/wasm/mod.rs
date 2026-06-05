pub mod instance;
pub mod linker;

use interstice_abi::ModuleSchema;
use std::sync::Arc;

use crate::runtime::Runtime;

pub struct StoreState {
    pub runtime: Arc<Runtime>,
    pub module_schema: Arc<ModuleSchema>,
}

