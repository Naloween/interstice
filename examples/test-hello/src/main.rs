use interstice_core::runtime::{Module, Runtime};
use interstice_core::wasm::{engine::WasmEngine, hostcalls::add_hostcalls};
use wasmtime::{Linker, Module as wasmtimeModule};

fn main() -> anyhow::Result<()> {
    let engine = WasmEngine::new();
    let mut linker = Linker::new(&engine.engine);
    add_hostcalls(&mut linker)?;

    let module = wasmtimeModule::from_file(
        &engine.engine,
        "../../modules/hello/target/wasm32-unknown-unknown/release/hello.wasm",
    )?;

    let mut store = engine.new_store(());
    let instance = linker.instantiate(&mut store, &module)?;

    let wasm_instance = interstice_core::wasm::instance::WasmInstance::new(store, instance)?;

    let mut runtime = Runtime::new();

    runtime.register_module(Module::new(
        "hello".into(),
        wasm_instance,
        vec!["hello".into()],
    ))?;

    let result = runtime.call_reducer("hello", "hello", b"Interstice")?;

    println!("Result: {}", String::from_utf8_lossy(&result));
    Ok(())
}
