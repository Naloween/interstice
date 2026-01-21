use crate::wasm::StoreState;
use wasmtime::{Caller, Linker};

pub fn define_host_calls(linker: &mut Linker<StoreState>) -> anyhow::Result<()> {
    linker.func_wrap(
        "interstice",
        "host_call",
        |mut caller: Caller<'_, StoreState>, call_id: u32, ptr: i32, len: i32| -> i64 {
            let memory = match caller.get_export("memory") {
                Some(wasmtime::Extern::Memory(mem)) => mem,
                _ => return 0, // hard trap later
            };

            let data = caller.data();
            let runtime = unsafe { &mut *data.runtime };
            let module_id = data.module_id;

            runtime.dispatch_host_call(module_id, &memory, &mut caller, call_id, ptr, len)
        },
    )?;

    Ok(())
}
