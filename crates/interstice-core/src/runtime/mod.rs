pub mod module;
pub mod reducer;

use interstice_abi::{ABI_VERSION, PrimitiveValue, types::ModuleId};
// optional re-exports (recommended)
pub use module::Module;
use wasmtime::{Engine, Linker, Module as wasmtimeModule, Store};

use crate::{
    error::IntersticeError,
    runtime::reducer::ReducerFrame,
    wasm::{StoreState, instance::WasmInstance, linker::define_host_calls},
};
use std::{collections::HashMap, path::Path, sync::Arc};

pub struct Runtime {
    pub modules: HashMap<String, Module>,
    pub call_stack: Vec<ReducerFrame>,
    pub engine: Arc<Engine>,
    pub linker: Linker<StoreState>,
    next_module_id: ModuleId,
}

impl Runtime {
    pub fn new() -> Self {
        let engine = Arc::new(Engine::default());
        let mut linker = Linker::new(&engine);
        define_host_calls(&mut linker).unwrap();
        Self {
            modules: HashMap::new(),
            call_stack: Vec::new(),
            engine,
            linker,
            next_module_id: 0,
        }
    }

    pub fn load_module<P: AsRef<Path>>(&mut self, path: P) -> Result<(), IntersticeError> {
        // Create wasm instance from provided file
        let wasm_module = wasmtimeModule::from_file(&self.engine, path).unwrap();
        let runtime_ptr: *mut Runtime = self;
        let mut store = Store::new(
            &self.engine,
            StoreState {
                runtime: runtime_ptr,
                module_id: self.next_module_id,
            },
        );
        self.next_module_id += 1;
        let instance = self.linker.instantiate(&mut store, &wasm_module).unwrap();
        let mut instance = WasmInstance::new(store, instance)?;

        // Generate schema
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

        // Create and register module
        let module = Module::new(instance, schema);
        self.modules.insert(module.schema.name.clone(), module);

        Ok(())
    }

    pub fn call(
        &mut self,
        module: &str,
        reducer: &str,
        input: PrimitiveValue,
    ) -> Result<PrimitiveValue, IntersticeError> {
        todo!();
    }
}
