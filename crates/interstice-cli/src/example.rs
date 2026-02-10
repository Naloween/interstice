use std::{fs::File, io::Write as _};

use interstice_core::{IntersticeError, Node};

use crate::{
    data_directory::nodes_dir,
    node_registry::{NodeRecord, NodeRegistry},
};

pub async fn example(port: u32) -> Result<(), IntersticeError> {
    let mut node = Node::new(&nodes_dir(), port)?;
    let mut registry = NodeRegistry::load()?;
    let name = format!("example-{}", port);
    if registry.get(&name).is_none() {
        registry.add(NodeRecord {
            name,
            address: format!("127.0.0.1:{}", port),
            node_id: Some(node.id.to_string()),
            local: true,
            last_seen: None,
            elusive: false,
        })?;
    }
    node.clear_logs().await.expect("Couldn't clear logs");
    let hello_bytes = include_bytes!("../../../target/wasm32-unknown-unknown/debug/hello.wasm");
    let caller_bytes = include_bytes!("../../../target/wasm32-unknown-unknown/debug/caller.wasm");
    let graphics_bytes =
        include_bytes!("../../../target/wasm32-unknown-unknown/debug/graphics.wasm");

    if port != 8080 {
        // Client
        let _caller_schema = node.load_module_from_bytes(caller_bytes).await?.to_public();
        let _graphics_schema = node
            .load_module_from_bytes(graphics_bytes)
            .await?
            .to_public();
    } else {
        // Server
        let hello_schema = node.load_module_from_bytes(hello_bytes).await?; //.to_public();
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
