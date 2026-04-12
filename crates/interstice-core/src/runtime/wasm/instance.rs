use super::StoreState;
use crate::error::IntersticeError;
use interstice_abi::{IntersticeValue, ModuleSchema, decode, encode};
use serde::Serialize;
use std::collections::HashMap;
use wasmtime::{Func, Instance, Memory, Store};

pub struct WasmInstance {
    pub store: Store<StoreState>,
    instance: Instance,
    memory: Memory,
    alloc: Func,
    dealloc: Func,
    /// Cached function handles to avoid per-call `instance.get_func` HashMap lookups.
    func_cache: HashMap<String, Func>,
    /// Pre-allocated scratch buffer in WASM memory, reused across every call.
    /// Eliminates the alloc/dealloc WASM calls (and their fiber switches) on the hot path.
    scratch_ptr: i32,
    scratch_capacity: usize,
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
            func_cache: HashMap::new(),
            scratch_ptr: 0,
            scratch_capacity: 0,
        })
    }

    /// Pre-cache function handles by name so that `call_reducer` / `call_query` skip the
    /// per-call `instance.get_func` lookup. Call this once after the schema is known.
    pub fn preload_funcs(&mut self, names: &[String]) {
        for name in names {
            if let Some(func) = self.instance.get_func(&mut self.store, name) {
                self.func_cache.insert(name.clone(), func);
            }
        }
    }

    /// Allocate an initial scratch buffer in WASM memory. After this, most calls will
    /// reuse the buffer without any WASM allocation round-trips.
    pub fn init_scratch(&mut self, capacity: usize) -> Result<(), IntersticeError> {
        if capacity == 0 {
            return Ok(());
        }
        let alloc = self
            .alloc
            .typed::<i32, i32>(&self.store)
            .map_err(|_| IntersticeError::BadSignature("alloc".into()))?;
        let ptr = alloc
            .call(&mut self.store, capacity as i32)
            .map_err(|e| IntersticeError::WasmTrap(e.to_string()))?;
        self.scratch_ptr = ptr;
        self.scratch_capacity = capacity;
        Ok(())
    }

    /// Sync version of grow_scratch — used from the reducer hot path (dedicated std::thread).
    fn grow_scratch_sync(&mut self, needed: usize) -> Result<(), IntersticeError> {
        if self.scratch_capacity > 0 {
            let dealloc = self
                .dealloc
                .typed::<(i32, i32), ()>(&self.store)
                .map_err(|_| IntersticeError::BadSignature("dealloc".into()))?;
            let _ = dealloc.call(&mut self.store, (self.scratch_ptr, self.scratch_capacity as i32));
        }
        let new_cap = needed.next_power_of_two().max(4096);
        let alloc = self
            .alloc
            .typed::<i32, i32>(&self.store)
            .map_err(|_| IntersticeError::BadSignature("alloc".into()))?;
        let ptr = alloc
            .call(&mut self.store, new_cap as i32)
            .map_err(|e| IntersticeError::WasmTrap(e.to_string()))?;
        self.scratch_ptr = ptr;
        self.scratch_capacity = new_cap;
        Ok(())
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

        if len < 0 {
            return Err(IntersticeError::MemoryRead);
        }
        if len == 0 {
            return Err(IntersticeError::Internal(
                "interstice_describe returned empty schema payload".to_string(),
            ));
        }

        let mut bytes = vec![0u8; len as usize];
        self.memory
            .read(&mut self.store, (ptr as u32) as usize, &mut bytes)
            .map_err(|_| IntersticeError::MemoryRead)?;

        let dealloc = self
            .dealloc
            .typed::<(i32, i32), ()>(&self.store)
            .map_err(|_| IntersticeError::BadSignature("dealloc".into()))?;
        let _ = dealloc.call(&mut self.store, (ptr, len));

        let schema: ModuleSchema = decode(&bytes).map_err(|err| {
            IntersticeError::Internal(format!(
                "Invalid schema payload (ptr={}, len={}): {}",
                ptr, len, err
            ))
        })?;

        Ok(schema)
    }

    pub fn call_reducer(
        &mut self,
        func_name: &str,
        args: impl Serialize,
    ) -> Result<(), IntersticeError> {
        let args_bytes = encode(&args).map_err(|err| {
            IntersticeError::Internal(format!("failed to serialize reducer arguments: {}", err))
        })?;

        // Grow scratch buffer only if args don't fit.
        if args_bytes.len() > self.scratch_capacity {
            self.grow_scratch_sync(args_bytes.len())?;
        }

        // Write args directly into the pre-allocated scratch buffer.
        self.memory
            .write(
                &mut self.store,
                (self.scratch_ptr as u32) as usize,
                &args_bytes,
            )
            .map_err(|_| IntersticeError::MemoryWrite)?;

        // Resolve func from cache, falling back to instance lookup for uncached names.
        let func = self
            .func_cache
            .get(func_name)
            .cloned()
            .or_else(|| self.instance.get_func(&mut self.store, func_name))
            .ok_or_else(|| IntersticeError::WasmFuncNotFound(func_name.into()))?;

        let reducer = func
            .typed::<(i32, i32), ()>(&self.store)
            .map_err(|_| IntersticeError::BadSignature(func_name.into()))?;

        reducer
            .call(
                &mut self.store,
                (self.scratch_ptr, args_bytes.len() as i32),
            )
            .map_err(|e| IntersticeError::WasmTrap(e.to_string()))?;

        Ok(())
    }

    pub fn call_query(
        &mut self,
        func_name: &str,
        args: impl Serialize,
    ) -> Result<IntersticeValue, IntersticeError> {
        let args_bytes = encode(&args).map_err(|err| {
            IntersticeError::Internal(format!("failed to serialize query arguments: {}", err))
        })?;

        if args_bytes.len() > self.scratch_capacity {
            self.grow_scratch_sync(args_bytes.len())?;
        }

        self.memory
            .write(
                &mut self.store,
                (self.scratch_ptr as u32) as usize,
                &args_bytes,
            )
            .map_err(|_| IntersticeError::MemoryWrite)?;

        let func = self
            .func_cache
            .get(func_name)
            .cloned()
            .or_else(|| self.instance.get_func(&mut self.store, func_name))
            .ok_or_else(|| IntersticeError::WasmFuncNotFound(func_name.into()))?;

        let query = func
            .typed::<(i32, i32), i64>(&self.store)
            .map_err(|_| IntersticeError::BadSignature(func_name.into()))?;

        let packed = query
            .call(
                &mut self.store,
                (self.scratch_ptr, args_bytes.len() as i32),
            )
            .map_err(|e| IntersticeError::WasmTrap(e.to_string()))?;

        let res_ptr = (packed >> 32) as i32;
        let res_len = (packed & 0xffffffff) as i32;

        if res_len < 0 {
            return Err(IntersticeError::MemoryRead);
        }

        let mut out = vec![0u8; res_len as usize];
        self.memory
            .read(&mut self.store, (res_ptr as u32) as usize, &mut out)
            .map_err(|_| IntersticeError::MemoryRead)?;

        // Free the module-allocated output buffer.
        let dealloc = self
            .dealloc
            .typed::<(i32, i32), ()>(&self.store)
            .map_err(|_| IntersticeError::BadSignature("dealloc".into()))?;
        let _ = dealloc.call(&mut self.store, (res_ptr, res_len));

        let out = decode(&out).map_err(|err| {
            IntersticeError::Internal(format!("failed to deserialize query output: {}", err))
        })?;

        Ok(out)
    }
}
