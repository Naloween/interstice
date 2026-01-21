pub mod module;
pub mod reducer;

use interstice_abi::{ABI_VERSION, types::ModuleId};
// optional re-exports (recommended)
pub use module::Module;

use crate::{error::IntersticeError, runtime::reducer::ReducerFrame, wasm::instance::WasmInstance};
use std::collections::HashMap;

pub struct Runtime {
    pub modules: HashMap<String, Module>,
    pub call_stack: Vec<ReducerFrame>,
}

pub struct ExecutionContext {
    reducer_stack: Vec<(ModuleId, String)>,
}

impl Runtime {
    pub fn new() -> Self {
        Self {
            modules: HashMap::new(),
            call_stack: Vec::new(),
        }
    }

    pub fn register_module(&mut self, mut instance: WasmInstance) -> Result<(), IntersticeError> {
        let schema = instance.load_schema()?;

        if schema.abi_version != ABI_VERSION {
            return Err(IntersticeError::AbiVersionMismatch {
                expected: ABI_VERSION,
                found: schema.abi_version,
            });
        }

        if self.modules.contains_key(&schema.name) {
            return Err(IntersticeError::ModuleAlreadyExists(schema.name));
        }

        let module = Module::new(instance, schema);
        self.modules.insert(module.schema.name.clone(), module);

        Ok(())
    }
}
