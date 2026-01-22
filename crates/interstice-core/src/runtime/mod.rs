pub mod module;
pub mod reducer;
pub mod table;

use crate::{
    runtime::{module::Module, reducer::ReducerFrame},
    wasm::{StoreState, linker::define_host_calls},
};
use std::{collections::HashMap, sync::Arc};
use wasmtime::{Engine, Linker};

pub struct Runtime {
    pub modules: HashMap<String, Module>,
    pub call_stack: Vec<ReducerFrame>,
    pub engine: Arc<Engine>,
    pub linker: Linker<StoreState>,
}

impl Runtime {
    pub fn new() -> Self {
        let engine = Arc::new(Engine::default());
        let mut linker = Linker::new(&engine);
        define_host_calls(&mut linker).expect("Couldn't add host calls to the linker");
        Self {
            modules: HashMap::new(),
            call_stack: Vec::new(),
            engine,
            linker,
        }
    }
}
