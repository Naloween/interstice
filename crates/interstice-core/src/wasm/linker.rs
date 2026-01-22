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

            let data = caller.data();
            let module_name = data.module_name.clone();
            let runtime = unsafe { &mut *data.runtime };

            match runtime.dispatch_host_call(&memory, &mut caller, module_name, ptr, len) {
                Ok(Some(result)) => result,
                Ok(None) => 0,
                Err(err) => {
                    println!("An error occured when dispatching the host call: {}", err);
                    0
                }
            }
        },
    )?;

    Ok(())
}
