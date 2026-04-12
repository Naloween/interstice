use crate::{
    data_directory::nodes_dir,
    node_registry::{NodeRecord, NodeRegistry},
    node_utils,
};
use interstice_core::{IntersticeError, Node};

const HELLO_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/module_examples/hello_example.wasm"
));
const CALLER_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/module_examples/caller_example.wasm"
));
const GRAPHICS_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/module_examples/graphics_example.wasm"
));
const AUDIO_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/module_examples/audio_example.wasm"
));
const AGAR_SERVER_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/module_examples/agar_server.wasm"
));
const AGAR_CLIENT_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/module_examples/agar_client.wasm"
));
const BENCHMARK_WORKLOAD_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/module_examples/benchmark_workload.wasm"
));
const DEFAULT_GRAPHICS_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/module_defaults/graphics.wasm"
));
const DEFAULT_INPUT_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/module_defaults/input.wasm"
));

struct ExampleModule {
    bytes: &'static [u8],
}

struct ExampleConfig {
    name: &'static str,
    port: u32,
    modules: Vec<ExampleModule>,
}

fn example_config(example_name: &str) -> Result<ExampleConfig, IntersticeError> {
    match example_name {
        "hello" => Ok(ExampleConfig {
            name: "hello-example",
            port: 8080,
            modules: vec![ExampleModule { bytes: HELLO_BYTES }],
        }),
        "caller" => Ok(ExampleConfig {
            name: "caller-example",
            port: 8081,
            modules: vec![ExampleModule {
                bytes: CALLER_BYTES,
            }],
        }),
        "graphics" => Ok(ExampleConfig {
            name: "graphics-example",
            port: 8082,
            modules: vec![ExampleModule {
                bytes: GRAPHICS_BYTES,
            }],
        }),
        "audio" => Ok(ExampleConfig {
            name: "audio-example",
            port: 8083,
            modules: vec![ExampleModule { bytes: AUDIO_BYTES }],
        }),
        "agar-server" => Ok(ExampleConfig {
            name: "agar-server-example",
            // Dedicated port: 8080 is used by hello-example; sharing it caused bind failures.
            port: 8086,
            modules: vec![ExampleModule {
                bytes: AGAR_SERVER_BYTES,
            }],
        }),
        "agar-client" => Ok(ExampleConfig {
            name: "agar-client-example",
            port: 8084,
            modules: vec![
                ExampleModule {
                    bytes: DEFAULT_INPUT_BYTES,
                },
                ExampleModule {
                    bytes: DEFAULT_GRAPHICS_BYTES,
                },
                ExampleModule {
                    bytes: AGAR_CLIENT_BYTES,
                },
            ],
        }),
        "benchmark" => Ok(ExampleConfig {
            name: "benchmark-example",
            port: 8085,
            modules: vec![ExampleModule {
                bytes: BENCHMARK_WORKLOAD_BYTES,
            }],
        }),
        _ => Err(IntersticeError::Internal(format!(
            "Unknown example '{example_name}'. Expected hello, caller, graphics, audio, agar-server, agar-client, or benchmark."
        ))),
    }
}

pub async fn example(example_name: &str) -> Result<(), IntersticeError> {
    let config = example_config(example_name)?;
    let mut registry = NodeRegistry::load()?;
    let name = config.name.to_string();

    // Remove existing node if present to ensure clean state
    if registry.get(&name).is_some() {
        node_utils::remove_node_with_data(&mut registry, &name)?;
    }

    // Create new node
    let node = Node::new(&nodes_dir(), config.port)?;
    registry.add(NodeRecord {
        name,
        address: format!("127.0.0.1:{}", config.port),
        node_id: Some(node.id.to_string()),
        local: true,
        last_seen: None,
    })?;

    // Write module bytes directly to disk (node.start() will load them)
    let modules_path = nodes_dir().join(node.id.to_string()).join("modules");
    for (idx, module_config) in config.modules.iter().enumerate() {
        // Use a simple directory name (actual module name will be read from WASM)
        let module_dir = modules_path.join(format!("module_{}", idx));
        std::fs::create_dir_all(&module_dir).map_err(|err| {
            IntersticeError::Internal(format!("Failed to create module directory: {err}"))
        })?;
        std::fs::write(module_dir.join("module.wasm"), module_config.bytes).map_err(|err| {
            IntersticeError::Internal(format!("Failed to write module WASM: {err}"))
        })?;
    }

    // Now start the node, which will load modules from disk
    node.start().await?;
    Ok(())
}
