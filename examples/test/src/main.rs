use interstice_abi::IntersticeValue;
use interstice_core::Runtime;

fn main() -> anyhow::Result<()> {
    let mut runtime = Runtime::new();

    let hello_path = "../../target/wasm32-unknown-unknown/debug/hello.wasm";
    // let caller_path = "../../target/wasm32-unknown-unknown/debug/caller.wasm";

    runtime.load_module(hello_path)?;
    // runtime.load_module(caller_path)?;

    runtime.run(
        "hello",
        "hello",
        IntersticeValue::Vec(vec![IntersticeValue::String("Naloween !".to_string())]),
    )?;
    // runtime.run("caller", "caller", IntersticeValue::Option(None))?;
    Ok(())
}
