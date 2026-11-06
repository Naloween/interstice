use crate::{
    data_directory::nodes_dir,
    node_registry::{NodeRecord, NodeRegistry},
    node_utils,
};
use interstice_core::{
    IntersticeError, Node,
    bundle::{Bundle, BundleManifest, BundleModule},
    interstice_abi::ABI_VERSION,
};

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
const DEFAULT_GRAPHICS_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/module_defaults/graphics.wasm"
));
const DEFAULT_INPUT_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/module_defaults/input.wasm"
));
const DEFAULT_MODULE_MANAGER_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/module_defaults/module_manager.wasm"
));
const DEFAULT_AUDIO_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/module_defaults/audio.wasm"
));
const DEFAULT_NETWORK_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/module_defaults/network.wasm"
));
const UI_EXAMPLE_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/module_examples/ui_example.wasm"
));
const DESKTOP_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/module_examples/desktop_example.wasm"
));
const HTTP_GET_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/module_examples/http_get_example.wasm"
));
const BROWSER_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/module_examples/browser_example.wasm"
));

const NOTES_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/module_examples/notes.wasm"
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
                ExampleModule { bytes: DEFAULT_INPUT_BYTES },
                ExampleModule { bytes: DEFAULT_GRAPHICS_BYTES },
                ExampleModule { bytes: AGAR_CLIENT_BYTES },
            ],
        }),
        "ui" => Ok(ExampleConfig {
            name: "ui-example",
            port: 8088,
            modules: vec![
                ExampleModule { bytes: DEFAULT_INPUT_BYTES },
                ExampleModule { bytes: DEFAULT_GRAPHICS_BYTES },
                ExampleModule { bytes: UI_EXAMPLE_BYTES },
            ],
        }),
        "desktop" => Ok(ExampleConfig {
            name: "desktop-example",
            port: 8089,
            modules: vec![
                ExampleModule {
                    bytes: DEFAULT_INPUT_BYTES,
                },
                ExampleModule {
                    bytes: DEFAULT_GRAPHICS_BYTES,
                },
                ExampleModule {
                    bytes: DEFAULT_MODULE_MANAGER_BYTES,
                },
                ExampleModule {
                    bytes: DEFAULT_NETWORK_BYTES,
                },
                ExampleModule {
                    bytes: DESKTOP_BYTES,
                },
            ],
        }),
        "http-get" => Ok(ExampleConfig {
            name: "http-get-example",
            port: 8090,
            modules: vec![
                ExampleModule {
                    bytes: DEFAULT_NETWORK_BYTES,
                },
                ExampleModule {
                    bytes: HTTP_GET_BYTES,
                },
            ],
        }),
        "browser" => Ok(ExampleConfig {
            name: "browser-example",
            port: 8091,
            modules: vec![
                ExampleModule {
                    bytes: DEFAULT_INPUT_BYTES,
                },
                ExampleModule {
                    bytes: DEFAULT_GRAPHICS_BYTES,
                },
                ExampleModule {
                    bytes: DEFAULT_NETWORK_BYTES,
                },
                ExampleModule {
                    bytes: BROWSER_BYTES,
                },
            ],
        }),
        "notes" => Ok(ExampleConfig {
            name: "notes-example",
            port: 8092,
            modules: vec![
                ExampleModule { bytes: DEFAULT_INPUT_BYTES },
                ExampleModule { bytes: DEFAULT_GRAPHICS_BYTES },
                ExampleModule { bytes: HELLO_BYTES },
                ExampleModule { bytes: NOTES_BYTES },
            ],
        }),
        _ => Err(IntersticeError::Internal(format!(
            "Unknown example '{example_name}'. Expected hello, caller, graphics, audio, agar-server, agar-client, ui, notes, desktop, http-get or browser."
        ))),
    }
}

/// The default modules shipped in the CLI, for `builtin:<name>` in a bundle
/// manifest (DESIGN §9.1).
pub fn builtin_module(name: &str) -> Option<&'static [u8]> {
    match name {
        "input" => Some(DEFAULT_INPUT_BYTES),
        "graphics" => Some(DEFAULT_GRAPHICS_BYTES),
        "audio" => Some(DEFAULT_AUDIO_BYTES),
        "module_manager" | "module-manager" => Some(DEFAULT_MODULE_MANAGER_BYTES),
        "network" => Some(DEFAULT_NETWORK_BYTES),
        _ => None,
    }
}

pub const BUILTIN_NAMES: &str = "input, graphics, audio, module_manager, network";

/// An example as an in-memory bundle. Module names are left to the runtime.
pub fn example_bundle(example_name: &str) -> Result<Bundle, IntersticeError> {
    let config = example_config(example_name)?;
    Ok(Bundle {
        manifest: BundleManifest {
            name: config.name.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            description: format!("The '{example_name}' example"),
            port: Some(config.port),
            abi_version: ABI_VERSION,
        },
        modules: config
            .modules
            .iter()
            .map(|m| BundleModule { name: None, wasm: m.bytes.to_vec(), data: Vec::new() })
            .collect(),
    })
}

pub async fn example(example_name: &str) -> Result<(), IntersticeError> {
    let bundle = example_bundle(example_name)?;
    let mut registry = NodeRegistry::load()?;
    let name = bundle.manifest.name.clone();
    let port = bundle.manifest.port.unwrap_or(8080);

    // Remove existing node if present to ensure clean state
    if registry.get(&name).is_some() {
        node_utils::remove_node_with_data(&mut registry, &name)?;
    }

    // Create new node
    let public_address = format!("127.0.0.1:{}", port);
    let node = Node::new(&nodes_dir(), port, public_address.clone())?;
    node_utils::own_node(&node)?;
    // An example's File authority works in the directory it runs from
    // (the browser's headless test files live there, DESIGN §7.4).
    let mut config = node.config()?;
    config.files_root = std::env::current_dir().ok();
    node.save_config(&config)?;
    registry.add(NodeRecord {
        name,
        address: public_address,
        node_id: Some(node.id.to_string()),
        local: true,
        last_seen: None,
    })?;

    // An example is a bundle installed fresh (DESIGN §9.1). Each load
    // persists the module to its name-based directory, so a later restart
    // (`node start <name>`) picks it up from disk.
    node.start_bundle(bundle, None).await?;
    Ok(())
}
