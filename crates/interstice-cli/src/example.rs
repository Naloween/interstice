use std::{fs::File, io::Write as _, path::Path};

use interstice_core::{IntersticeError, Node};

use crate::data_directory::data_file;

pub async fn example(port: u32) -> Result<(), IntersticeError> {
    let mut node = Node::new(&data_file(), port)?;
    node.clear_logs().await.expect("Couldn't clear logs");
    let hello_path = "../../target/wasm32-unknown-unknown/debug/hello.wasm";
    let caller_path = "../../target/wasm32-unknown-unknown/debug/caller.wasm";
    let graphics_path = "../../target/wasm32-unknown-unknown/debug/graphics.wasm";

    if port != 8080 {
        // Client
        let _caller_schema = node.load_module_from_file(caller_path).await?.to_public();
        let _graphics_schema = node.load_module_from_file(graphics_path).await?.to_public();
    } else {
        // Server
        let hello_schema = node.load_module_from_file(hello_path).await?; //.to_public();
        File::create("./hello_schema.toml")
            .unwrap()
            .write_all(&hello_schema.to_toml_string().unwrap().as_bytes())
            .unwrap();
    }

    let node_schema = node.schema("MyNode".into()).await.to_public();
    File::create("./node_schema.toml")
        .unwrap()
        .write_all(&node_schema.to_toml_string().unwrap().as_bytes())
        .unwrap();

    node.start().await?;
    Ok(())
}
