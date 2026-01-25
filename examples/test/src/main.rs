use std::path::Path;

use interstice_core::{
    interstice_abi::{IntersticeValue, ModuleSchema},
    *,
};

fn main() -> anyhow::Result<()> {
    let mut runtime =
        Runtime::new(Path::new("./transactions.log")).expect("Couldn't create runtime");
    runtime.clear_logs().expect("Couldn't clear logs");

    let hello_path = "../../target/wasm32-unknown-unknown/debug/hello.wasm";
    let caller_path = "../../target/wasm32-unknown-unknown/debug/caller.wasm";

    let hello_schema = runtime.load_module(hello_path)?;
    let caller_schema = runtime.load_module(caller_path)?;

    println!("{}", hello_schema.to_toml_string().unwrap());
    println!("{}", caller_schema.to_toml_string().unwrap());

    runtime.run(
        "hello",
        "hello",
        IntersticeValue::Vec(vec![IntersticeValue::String("Naloween !".to_string())]),
    )?;
    runtime.run("caller", "caller", IntersticeValue::Vec(vec![]))?;
    Ok(())
}
