use wasmtime::{Caller, Linker};

pub fn add_hostcalls(linker: &mut Linker<()>) -> anyhow::Result<()> {
    linker.func_wrap(
        "interstice",
        "log",
        |mut caller: Caller<'_, ()>, ptr: i32, len: i32| {
            // TODO: read memory and print
            println!("[wasm] log called");
        },
    )?;
    Ok(())
}
