use crate::{
    data_directory::nodes_dir,
    node_registry::{NodeRecord, NodeRegistry},
};
use interstice_core::{IntersticeError, Node};

const HELLO_BYTES: &[u8] =
    include_bytes!("../../../target/wasm32-unknown-unknown/debug/hello.wasm");
const CALLER_BYTES: &[u8] =
    include_bytes!("../../../target/wasm32-unknown-unknown/debug/caller.wasm");
const GRAPHICS_BYTES: &[u8] =
    include_bytes!("../../../target/wasm32-unknown-unknown/debug/graphics.wasm");
const AUDIO_BYTES: &[u8] =
    include_bytes!("../../../target/wasm32-unknown-unknown/debug/audio.wasm");

struct ExampleModule {
    bytes: &'static [u8],
    public: bool,
}

struct ExampleConfig {
    name: &'static str,
    port: u32,
    modules: Vec<ExampleModule>,
}

fn example_config(example_name: &str) -> Result<ExampleConfig, IntersticeError> {
    match example_name {
        "hello" => Ok(ExampleConfig {
            name: "hello",
            port: 8080,
            modules: vec![ExampleModule {
                bytes: HELLO_BYTES,
                public: false,
            }],
        }),
        "caller" => Ok(ExampleConfig {
            name: "caller",
            port: 8081,
            modules: vec![ExampleModule {
                bytes: CALLER_BYTES,
                public: true,
            }],
        }),
        "graphics" => Ok(ExampleConfig {
            name: "graphics",
            port: 8082,
            modules: vec![ExampleModule {
                bytes: GRAPHICS_BYTES,
                public: true,
            }],
        }),
        "audio" => Ok(ExampleConfig {
            name: "audio",
            port: 8083,
            modules: vec![ExampleModule {
                bytes: AUDIO_BYTES,
                public: false,
            }],
        }),
        _ => Err(IntersticeError::Internal(format!(
            "Unknown example '{example_name}'. Expected hello, caller, graphics, or audio."
        ))),
    }
}

pub async fn example(example_name: &str) -> Result<(), IntersticeError> {
    let config = example_config(example_name)?;
    let mut registry = NodeRegistry::load()?;
    let name = format!("example-{}", config.name);
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
        if node_port != config.port {
            return Err(IntersticeError::Internal(format!(
                "Example '{}' expects port {}, but registry has {}. Remove the existing entry to continue.",
                config.name, config.port, node_port
            )));
        }
        (
            Node::load(&nodes_dir(), node_id.parse().unwrap(), node_port).await?,
            false,
        )
    } else {
        let node = Node::new(&nodes_dir(), config.port)?;
        registry.add(NodeRecord {
            name,
            address: format!("127.0.0.1:{}", config.port),
            node_id: Some(node.id.to_string()),
            local: true,
            last_seen: None,
        })?;
        (node, true)
    };

    let modules_path = nodes_dir().join(node.id.to_string()).join("modules");
    let has_modules = std::fs::read_dir(&modules_path)
        .map(|entries| {
            entries.filter_map(|entry| entry.ok()).any(|entry| {
                let path = entry.path();
                path.is_dir() && path.join("module.wasm").exists()
            })
        })
        .unwrap_or(false);

    if is_new || !has_modules {
        for module in &config.modules {
            let schema = node.load_module_from_bytes(module.bytes).await?;
            if module.public {
                let _ = schema.to_public();
            }
        }
    }

    let _node_schema = node.schema("MyNode".into()).await.to_public();

    node.start().await?;
    Ok(())
}
