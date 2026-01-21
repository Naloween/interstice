use wasmtime::{Engine, Store};

pub struct WasmEngine {
    pub engine: Engine,
}

impl WasmEngine {
    pub fn new() -> Self {
        let engine = Engine::default();
        Self { engine }
    }

    pub fn new_store<T>(&self, data: T) -> Store<T> {
        Store::new(&self.engine, data)
    }
}
