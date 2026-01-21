use crate::{error::IntersticeError, wasm::instance::WasmInstance};
use interstice_abi::{ModuleSchema, PrimitiveValue};

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

    pub fn call_reducer(
        &mut self,
        reducer: &str,
        input: PrimitiveValue,
    ) -> Result<PrimitiveValue, IntersticeError> {
        return self.instance.call_reducer(reducer, input);
    }
}
