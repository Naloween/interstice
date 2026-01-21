use interstice_abi::PrimitiveValue;
use interstice_core::runtime::Runtime;

fn main() -> anyhow::Result<()> {
    let mut runtime = Runtime::new();

    let path = "../../modules/hello/target/wasm32-unknown-unknown/release/hello.wasm";

    runtime.load_module(path)?;

    let result = runtime.invoke_reducer(
        "hello",
        "hello",
        PrimitiveValue::String("Interstice".into()),
    )?;
    if let PrimitiveValue::String(msg) = result {
        println!("Result: {}", msg);
    }
    Ok(())
}
