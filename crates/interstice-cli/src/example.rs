use std::{fs::File, io::Write as _};

use interstice_core::{IntersticeError, Node};

use crate::{
    data_directory::nodes_dir,
    node_registry::{NodeRecord, NodeRegistry},
};

pub async fn example(port: u32) -> Result<(), IntersticeError> {
    let mut registry = NodeRegistry::load()?;
    let name = format!("example-{}", port);
    let (node, is_new) = if let Some(entry) = registry.get(&name) {
        let node_id = entry
            .node_id
            .as_ref()
            .ok_or_else(|| IntersticeError::Internal("Missing node id".into()))?;
        let node_port = entry
            .address
            .split(':')
            .last()
            .ok_or_else(|| IntersticeError::Internal("Invalid address".into()))?
            .parse()
            .map_err(|_| IntersticeError::Internal("Invalid port".into()))?;
        (
            Node::load(&nodes_dir(), node_id.parse().unwrap(), node_port).await?,
            false,
        )
    } else {
        let node = Node::new(&nodes_dir(), port)?;
        registry.add(NodeRecord {
            name,
            address: format!("127.0.0.1:{}", port),
            node_id: Some(node.id.to_string()),
            local: true,
            last_seen: None,
            elusive: false,
        })?;
        (node, true)
    };

    let hello_bytes = include_bytes!("../../../target/wasm32-unknown-unknown/debug/hello.wasm");
    let caller_bytes = include_bytes!("../../../target/wasm32-unknown-unknown/debug/caller.wasm");
    let graphics_bytes =
        include_bytes!("../../../target/wasm32-unknown-unknown/debug/graphics.wasm");

    if is_new && port != 8080 {
        // Client
        let _caller_schema = node.load_module_from_bytes(caller_bytes).await?.to_public();
        let _graphics_schema = node
            .load_module_from_bytes(graphics_bytes)
            .await?
            .to_public();
    } else if is_new {
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
