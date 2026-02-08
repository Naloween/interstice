use super::StoreState;
use crate::logger::{LogLevel, LogSource};
use wasmtime::{Caller, Linker};

pub fn define_host_calls(linker: &mut Linker<StoreState>) -> anyhow::Result<()> {
    linker.func_wrap_async(
        "interstice",
        "interstice_host_call",
        |mut caller: Caller<'_, StoreState>, (ptr, len): (i32, i32)| {
            Box::new(async move {
            let memory = match caller.get_export("memory") {
                Some(wasmtime::Extern::Memory(mem)) => mem,
                _ => return 0, // hard trap later
            };

            let data = caller.data();
            let module_schema = data.module_schema.clone();
            let runtime = data.runtime.clone();

            match runtime
                .dispatch_host_call(&memory, &mut caller, module_schema, ptr, len)
                .await
            {
                Ok(Some(result)) => result,
                Ok(None) => 0,
                Err(err) => {
                    runtime.logger.log(
                        &format!(
                            "An error occured when dispatching the host call: {}",
                            err
                        ),
                        LogSource::Runtime,
                        LogLevel::Error,
                    );
                    0
                }
            }
            })
        },
    )?;

    Ok(())
}
