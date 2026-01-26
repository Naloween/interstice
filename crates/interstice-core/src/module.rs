use crate::{
    Node,
    error::IntersticeError,
    table::Table,
    wasm::{StoreState, instance::WasmInstance},
};
use interstice_abi::{ABI_VERSION, IntersticeValue, ModuleSchema, ReducerContext};
use std::{collections::HashMap, path::Path};
use wasmtime::{Module as wasmtimeModule, Store};

pub struct Module {
    instance: WasmInstance,
    pub schema: ModuleSchema,
    pub tables: HashMap<String, Table>,
}

impl Module {
    pub fn new(mut instance: WasmInstance) -> Result<Self, IntersticeError> {
        let schema = instance.load_schema()?;
        if schema.abi_version != ABI_VERSION {
            return Err(IntersticeError::AbiVersionMismatch {
                expected: ABI_VERSION,
                found: schema.abi_version,
            });
        }

        let tables = schema
            .tables
            .iter()
            .map(|table_schema| {
                (
                    table_schema.name.clone(),
                    Table {
                        schema: table_schema.clone(),
                        rows: Vec::new(),
                    },
                )
            })
            .collect();

        // Set module name in the store state
        instance.store.data_mut().module_name = schema.name.clone();

        Ok(Self {
            instance,
            schema,
            tables,
        })
    }

    pub fn schema(&self) -> &ModuleSchema {
        &self.schema
    }

    pub fn call_reducer(
        &mut self,
        reducer: &str,
        args: (ReducerContext, IntersticeValue),
    ) -> Result<IntersticeValue, IntersticeError> {
        return self.instance.call_reducer(reducer, args);
    }
}

impl Node {
    pub fn load_module<P: AsRef<Path>>(
        &mut self,
        path: P,
    ) -> Result<ModuleSchema, IntersticeError> {
        // Create wasm instance from provided file
        let wasm_module = wasmtimeModule::from_file(&self.engine, path).unwrap();
        let runtime_ptr: *mut Node = self;
        let mut store = Store::new(
            &self.engine,
            StoreState {
                node: runtime_ptr,
                module_name: String::new(),
            },
        );
        let instance = self.linker.instantiate(&mut store, &wasm_module).unwrap();
        let instance = WasmInstance::new(store, instance)?;

        // Create and register module
        let module = Module::new(instance)?;
        let module_schema = module.schema.clone();
        // Add name to store
        if self.modules.contains_key(&module.schema.name) {
            return Err(IntersticeError::ModuleAlreadyExists(module.schema.name));
        }
        self.modules.insert(module.schema.name.clone(), module);

        Ok(module_schema)
    }
}
