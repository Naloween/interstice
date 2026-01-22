use interstice_abi::PrimitiveValue;
use interstice_core::runtime::Runtime;

fn main() -> anyhow::Result<()> {
    let mut runtime = Runtime::new();

    let hello_path = "../../target/wasm32-unknown-unknown/debug/hello.wasm";
    let caller_path = "../../target/wasm32-unknown-unknown/debug/caller.wasm";

    runtime.load_module(hello_path)?;
    runtime.load_module(caller_path)?;

    runtime.invoke_reducer(
        "hello",
        "hello",
        PrimitiveValue::String("Naloween !".to_string()),
    )?;
    runtime.invoke_reducer("caller", "caller", PrimitiveValue::Option(None))?;
    Ok(())
}
