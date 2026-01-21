use interstice_abi::PrimitiveValue;
use interstice_core::runtime::Runtime;
use interstice_core::wasm::{engine::WasmEngine, hostcalls::add_hostcalls};
use wasmtime::{Linker, Module as wasmtimeModule};

fn main() -> anyhow::Result<()> {
    let engine = WasmEngine::new();
    let mut linker = Linker::new(&engine.engine);
    add_hostcalls(&mut linker)?;

    let wasm_module = wasmtimeModule::from_file(
        &engine.engine,
        "../../modules/hello/target/wasm32-unknown-unknown/release/hello.wasm",
    )?;

    let mut store = engine.new_store(());
    let instance = linker.instantiate(&mut store, &wasm_module)?;

    let wasm_instance = interstice_core::wasm::instance::WasmInstance::new(store, instance)?;

    let mut runtime = Runtime::new();
    runtime.register_module(wasm_instance)?;

    let result = runtime.call_reducer(
        "hello",
        "hello",
        PrimitiveValue::String("Interstice".into()),
    )?;
    if let PrimitiveValue::String(msg) = result {
        println!("Result: {}", msg);
    }
    Ok(())
}
