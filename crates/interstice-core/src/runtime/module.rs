use crate::wasm::instance::WasmInstance;
use interstice_abi::module::ModuleSchema;

pub struct Module {
    instance: WasmInstance,
    pub schema: ModuleSchema,
}

impl Module {
    pub fn new(instance: WasmInstance, schema: ModuleSchema) -> Self {
        Self { instance, schema }
    }

    pub fn schema(&self) -> &ModuleSchema {
        &self.schema
    }

    pub fn instance_mut(&mut self) -> &mut WasmInstance {
        &mut self.instance
    }
}
