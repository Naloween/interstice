pub mod module;
pub mod reducer;

// optional re-exports (recommended)
pub use module::Module;

use crate::{error::IntersticeError, runtime::reducer::ReducerFrame};
use std::collections::HashMap;

pub struct Runtime {
    pub modules: HashMap<String, Module>,
    pub call_stack: Vec<ReducerFrame>,
}

impl Runtime {
    pub fn new() -> Self {
        Self {
            modules: HashMap::new(),
            call_stack: Vec::new(),
        }
    }

    pub fn register_module(&mut self, module: Module) -> Result<(), IntersticeError> {
        let name = &module.schema().name;
        if self.modules.contains_key(name) {
            return Err(IntersticeError::ModuleAlreadyExists(name.into()));
        }
        self.modules.insert(name.into(), module);
        Ok(())
    }
}
