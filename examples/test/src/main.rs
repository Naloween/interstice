use interstice_core::{interstice_abi::IntersticeValue, *};
use std::{fs::File, io::Write, path::Path};

fn main() -> anyhow::Result<()> {
    let mut runtime = Node::new(Path::new("./transactions.log")).expect("Couldn't create runtime");
    runtime.clear_logs().expect("Couldn't clear logs");

    let hello_path = "../../target/wasm32-unknown-unknown/debug/hello.wasm";
    let caller_path = "../../target/wasm32-unknown-unknown/debug/caller.wasm";

    let hello_schema = runtime.load_module(hello_path)?;
    let hello_schema_public = hello_schema.to_public();
    let caller_schema = runtime.load_module(caller_path)?;
    let caller_schema_public = hello_schema.to_public();

    File::create("./hello_schema.toml")
        .unwrap()
        .write_all(&hello_schema.to_toml_string().unwrap().as_bytes())
        .unwrap();
    File::create("./caller_schema.toml")
        .unwrap()
        .write_all(&caller_schema.to_toml_string().unwrap().as_bytes())
        .unwrap();
    File::create("./hello_schema_public.toml")
        .unwrap()
        .write_all(&hello_schema_public.to_toml_string().unwrap().as_bytes())
        .unwrap();
    File::create("./caller_schema_public.toml")
        .unwrap()
        .write_all(&caller_schema_public.to_toml_string().unwrap().as_bytes())
        .unwrap();

    runtime.run(
        "hello",
        "hello",
        IntersticeValue::Vec(vec![IntersticeValue::String("Naloween !".to_string())]),
    )?;
    runtime.run("caller", "caller", IntersticeValue::Vec(vec![]))?;
    // runtime.start()?;
    Ok(())
}
