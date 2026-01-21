use crate::wasm::instance::WasmInstance;

pub struct Module {
    instance: WasmInstance,
    pub schema: ModuleSchema,
}

impl Module {
    pub fn new(name: String, instance: WasmInstance, reducers: Vec<String>) -> Self {
        Self {
            instance,
            schema: ModuleSchema {
                name,
                version: 1,
                reducers,
            },
        }
    }

    pub fn schema(&self) -> &ModuleSchema {
        &self.schema
    }

    pub fn instance_mut(&mut self) -> &mut WasmInstance {
        &mut self.instance
    }
}

pub struct ModuleSchema {
    pub name: String,
    pub version: u32,
    pub reducers: Vec<String>,
}
