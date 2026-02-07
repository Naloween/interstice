use interstice_core::IntersticeError;

pub async fn publish() -> Result<(), IntersticeError> {
    // This should take a path to a rust project that it will build and publish. The module name is from Cargo.toml. It should build the project using cargo, then read the generated wasm file and send it to the node using the network module.
    // It should also be able to use saved servers nodes with their adress to easily publish to known nodes.
    println!("Publish command is not implemented yet");
    Ok(())
}
