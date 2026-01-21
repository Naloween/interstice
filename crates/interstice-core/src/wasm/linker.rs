use crate::wasm::StoreState;
use wasmtime::{Caller, Linker};

pub fn define_host_calls(linker: &mut Linker<StoreState>) -> anyhow::Result<()> {
    linker.func_wrap(
        "interstice",
        "interstice_host_call",
        |mut caller: Caller<'_, StoreState>, ptr: i32, len: i32| -> i64 {
            let memory = match caller.get_export("memory") {
                Some(wasmtime::Extern::Memory(mem)) => mem,
                _ => return 0, // hard trap later
            };

            println!("Host called");

            let data = caller.data();
            let runtime = unsafe { &mut *data.runtime };
            let module_id = data.module_id;

            runtime.dispatch_host_call(module_id, &memory, &mut caller, ptr, len)
        },
    )?;

    Ok(())
}
