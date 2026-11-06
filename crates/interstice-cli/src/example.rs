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

// The default modules, embedded (DESIGN §10).
const DEFAULT_GRAPHICS_BYTES: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/module_defaults/graphics.wasm"));
const DEFAULT_INPUT_BYTES: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/module_defaults/input.wasm"));
const DEFAULT_MODULE_MANAGER_BYTES: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/module_defaults/module_manager.wasm"));
const DEFAULT_AUDIO_BYTES: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/module_defaults/audio.wasm"));
const DEFAULT_NETWORK_BYTES: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/module_defaults/network.wasm"));

/// Where releases keep the example modules (`release-cli.yml`).
const RELEASES: &str = "https://github.com/Naloween/interstice/releases/download";

/// A module of an example: a default module (embedded), or an example's
/// wasm file, fetched (DESIGN §10).
enum ExampleModule {
    Builtin(&'static [u8]),
    Fetched(&'static str),
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
            modules: vec![ExampleModule::Fetched("hello_example.wasm")],
        }),
        "caller" => Ok(ExampleConfig {
            name: "caller-example",
            port: 8081,
            modules: vec![ExampleModule::Fetched("caller_example.wasm")],
        }),
        "graphics" => Ok(ExampleConfig {
            name: "graphics-example",
            port: 8082,
            modules: vec![ExampleModule::Fetched("graphics_example.wasm")],
        }),
        "audio" => Ok(ExampleConfig {
            name: "audio-example",
            port: 8083,
            modules: vec![ExampleModule::Fetched("audio_example.wasm")],
        }),
        "agar-server" => Ok(ExampleConfig {
            name: "agar-server-example",
            // Dedicated port: 8080 is used by hello-example; sharing it caused bind failures.
            port: 8086,
            modules: vec![ExampleModule::Fetched("agar_server.wasm")],
        }),
        "agar-client" => Ok(ExampleConfig {
            name: "agar-client-example",
            port: 8084,
            modules: vec![
                ExampleModule::Builtin(DEFAULT_INPUT_BYTES),
                ExampleModule::Builtin(DEFAULT_GRAPHICS_BYTES),
                ExampleModule::Fetched("agar_client.wasm"),
            ],
        }),
        "ui" => Ok(ExampleConfig {
            name: "ui-example",
            port: 8088,
            modules: vec![
                ExampleModule::Builtin(DEFAULT_INPUT_BYTES),
                ExampleModule::Builtin(DEFAULT_GRAPHICS_BYTES),
                ExampleModule::Fetched("ui_example.wasm"),
            ],
        }),
        "desktop" => Ok(ExampleConfig {
            name: "desktop-example",
            port: 8089,
            modules: vec![
                ExampleModule::Builtin(DEFAULT_INPUT_BYTES),
                ExampleModule::Builtin(DEFAULT_GRAPHICS_BYTES),
                ExampleModule::Builtin(DEFAULT_MODULE_MANAGER_BYTES),
                ExampleModule::Builtin(DEFAULT_NETWORK_BYTES),
                ExampleModule::Fetched("desktop_example.wasm"),
            ],
        }),
        "http-get" => Ok(ExampleConfig {
            name: "http-get-example",
            port: 8090,
            modules: vec![
                ExampleModule::Builtin(DEFAULT_NETWORK_BYTES),
                ExampleModule::Fetched("http_get_example.wasm"),
            ],
        }),
        "browser" => Ok(ExampleConfig {
            name: "browser-example",
            port: 8091,
            modules: vec![
                ExampleModule::Builtin(DEFAULT_INPUT_BYTES),
                ExampleModule::Builtin(DEFAULT_GRAPHICS_BYTES),
                ExampleModule::Builtin(DEFAULT_NETWORK_BYTES),
                ExampleModule::Fetched("browser_example.wasm"),
            ],
        }),
        "notes" => Ok(ExampleConfig {
            name: "notes-example",
            port: 8092,
            modules: vec![
                ExampleModule::Builtin(DEFAULT_INPUT_BYTES),
                ExampleModule::Builtin(DEFAULT_GRAPHICS_BYTES),
                ExampleModule::Fetched("hello_example.wasm"),
                ExampleModule::Fetched("notes.wasm"),
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

/// The bytes of an example's wasm `file`: from `INTERSTICE_EXAMPLES_DIR`,
/// the checkout's `dist/examples/`, or the cache, else downloaded from the
/// release of this CLI's version and cached (DESIGN §10).
async fn fetch_example(file: &str) -> Result<Vec<u8>, IntersticeError> {
    let version = env!("CARGO_PKG_VERSION");
    let cache = crate::data_directory::data_file().join("examples").join(version);
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    if let Some(dir) = std::env::var_os("INTERSTICE_EXAMPLES_DIR") {
        dirs.push(dir.into());
    }
    // A CLI built from a checkout uses the examples built next to it.
    dirs.push(concat!(env!("CARGO_MANIFEST_DIR"), "/../../dist/examples").into());
    dirs.push(cache.clone());
    for dir in &dirs {
        if let Ok(bytes) = std::fs::read(dir.join(file)) {
            return Ok(bytes);
        }
    }
    let url = format!("{RELEASES}/v{version}/{file}");
    println!("Downloading {file} ({version})...");
    let failed = |e: String| {
        IntersticeError::Internal(format!(
            "couldn't download {url}: {e}. From a checkout, build the examples with scripts/build_modules.sh"
        ))
    };
    let response = reqwest::get(&url).await.map_err(|e| failed(e.to_string()))?;
    if !response.status().is_success() {
        return Err(failed(response.status().to_string()));
    }
    let bytes = response.bytes().await.map_err(|e| failed(e.to_string()))?.to_vec();
    std::fs::create_dir_all(&cache).map_err(|e| IntersticeError::Internal(format!("{}: {e}", cache.display())))?;
    let tmp = cache.join(format!("{file}.part"));
    std::fs::write(&tmp, &bytes).and_then(|_| std::fs::rename(&tmp, cache.join(file))).map_err(|e| {
        IntersticeError::Internal(format!("{}: {e}", cache.display()))
    })?;
    Ok(bytes)
}

/// An example as an in-memory bundle. Module names are left to the runtime.
pub async fn example_bundle(example_name: &str) -> Result<Bundle, IntersticeError> {
    let config = example_config(example_name)?;
    let mut modules = Vec::with_capacity(config.modules.len());
    for module in &config.modules {
        let wasm = match module {
            ExampleModule::Builtin(bytes) => bytes.to_vec(),
            ExampleModule::Fetched(file) => fetch_example(file).await?,
        };
        modules.push(BundleModule { name: None, wasm, data: Vec::new() });
    }
    Ok(Bundle {
        manifest: BundleManifest {
            name: config.name.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            description: format!("The '{example_name}' example"),
            port: Some(config.port),
            abi_version: ABI_VERSION,
        },
        modules,
    })
}

pub async fn example(example_name: &str) -> Result<(), IntersticeError> {
    let bundle = example_bundle(example_name).await?;
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
