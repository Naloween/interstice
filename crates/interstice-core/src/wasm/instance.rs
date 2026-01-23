use crate::{error::IntersticeError, wasm::StoreState};
use interstice_abi::{ModuleSchema, IntersticeValue, decode, encode};
use wasmtime::{Func, Instance, Memory, Store};

pub struct WasmInstance {
    pub store: Store<StoreState>,
    instance: Instance,
    memory: Memory,
    alloc: Func,
    dealloc: Func,
}

impl WasmInstance {
    pub fn new(mut store: Store<StoreState>, instance: Instance) -> Result<Self, IntersticeError> {
        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or(IntersticeError::MissingExport("memory"))?;

        let alloc = instance
            .get_func(&mut store, "alloc")
            .ok_or(IntersticeError::MissingExport("alloc"))?;

        let dealloc = instance
            .get_func(&mut store, "dealloc")
            .ok_or(IntersticeError::MissingExport("dealloc"))?;

        Ok(Self {
            store,
            instance,
            memory,
            alloc,
            dealloc,
        })
    }

    pub fn load_schema(&mut self) -> Result<ModuleSchema, IntersticeError> {
        let func = self
            .instance
            .get_func(&mut self.store, "interstice_describe")
            .ok_or(IntersticeError::MissingExport("interstice_describe"))?;

        let typed = func
            .typed::<(), i64>(&self.store)
            .map_err(|_| IntersticeError::BadSignature("interstice_describe".into()))?;

        let packed = typed
            .call(&mut self.store, ())
            .map_err(|e| IntersticeError::WasmTrap(e.to_string()))?;

        let ptr = (packed >> 32) as i32;
        let len = (packed & 0xffffffff) as i32;

        let mut bytes = vec![0u8; len as usize];
        self.memory
            .read(&mut self.store, ptr as usize, &mut bytes)
            .map_err(|_| IntersticeError::MemoryRead)?;

        // IMPORTANT: module owns allocation → module must free
        let dealloc = self
            .dealloc
            .typed::<(i32, i32), ()>(&self.store)
            .map_err(|_| IntersticeError::BadSignature("dealloc".into()))?;

        let _ = dealloc.call(&mut self.store, (ptr, len));

        let schema = decode(&bytes).map_err(|_| IntersticeError::InvalidSchema)?;

        Ok(schema)
    }

    pub fn call_reducer(
        &mut self,
        func_name: &str,
        args: IntersticeValue,
    ) -> Result<IntersticeValue, IntersticeError> {
        let args_bytes = encode(&args)
            .map_err(|_| IntersticeError::Internal("failed to serialize reducer arguments"))?;

        let alloc = self
            .alloc
            .typed::<i32, i32>(&self.store)
            .map_err(|_| IntersticeError::BadSignature("alloc".into()))?;

        let dealloc = self
            .dealloc
            .typed::<(i32, i32), ()>(&self.store)
            .map_err(|_| IntersticeError::BadSignature("dealloc".into()))?;

        let func = self
            .instance
            .get_func(&mut self.store, func_name)
            .ok_or_else(|| IntersticeError::WasmFuncNotFound(func_name.into()))?;

        let reducer = func
            .typed::<(i32, i32), i64>(&self.store)
            .map_err(|_| IntersticeError::BadSignature(func_name.into()))?;

        // --- allocate input ---
        let ptr = alloc
            .call(&mut self.store, args_bytes.len() as i32)
            .map_err(|e| IntersticeError::WasmTrap(e.to_string()))?;

        // write args
        self.memory
            .write(&mut self.store, ptr as usize, &args_bytes)
            .map_err(|_| IntersticeError::MemoryWrite)?;

        // --- call reducer ---
        let packed = reducer
            .call(&mut self.store, (ptr, args_bytes.len() as i32))
            .map_err(|e| IntersticeError::WasmTrap(e.to_string()))?;

        // free input
        dealloc
            .call(&mut self.store, (ptr, args_bytes.len() as i32))
            .ok();

        // unpack result
        let res_ptr = (packed >> 32) as i32;
        let res_len = (packed & 0xffffffff) as i32;

        let mut out = vec![0u8; res_len as usize];
        self.memory
            .read(&mut self.store, res_ptr as usize, &mut out)
            .map_err(|_| IntersticeError::MemoryRead)?;

        // free output
        dealloc.call(&mut self.store, (res_ptr, res_len)).ok();

        let out = decode(&out)
            .map_err(|_| IntersticeError::Internal("failed to deserialize reducer output"))?;

        Ok(out)
    }
}
