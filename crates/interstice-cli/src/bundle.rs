//! `interstice bundle create|inspect|run|exe` (DESIGN §9.1).

use crate::{
    data_directory::nodes_dir,
    example::{BUILTIN_NAMES, builtin_module, example_bundle},
    module::{ask_decision, build_module_project},
    node_registry::{NodeRecord, NodeRegistry},
    node_utils,
};
use interstice_core::{
    Decide, IntersticeError, MigrationPolicy, Node, NodeId,
    bundle::{self, Bundle, BundleManifest, BundleModule},
    interstice_abi::ABI_VERSION,
};
use serde::Deserialize;
use std::path::{Path, PathBuf};

fn err(text: impl Into<String>) -> IntersticeError {
    IntersticeError::Internal(text.into())
}

/// `bundle.toml`, the manifest `bundle create` reads.
#[derive(Debug, Deserialize)]
struct ManifestFile {
    name: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    description: String,
    port: Option<u32>,
    /// `builtin:<name>`, a `.wasm` file, or a module project directory.
    modules: Vec<String>,
}

fn flag_value(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned()
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| a == flag)
}

pub async fn handle_bundle_command(args: &[String]) -> Result<(), IntersticeError> {
    let rest = &args[2.min(args.len())..];
    match rest.first().map(String::as_str) {
        Some("create") if rest.len() >= 3 => create(&rest[1..]).await,
        Some("inspect") if rest.len() >= 2 => inspect(Path::new(&rest[1])),
        Some("exe") if rest.len() >= 3 => exe(Path::new(&rest[1]), Path::new(&rest[2])),
        Some("run") if rest.len() >= 2 => {
            let bundle = Bundle::read(Path::new(&rest[1]))?;
            run_bundle(bundle, RunOptions::from_args(&rest[2..])?).await
        }
        _ => {
            print_bundle_help();
            Ok(())
        }
    }
}

pub fn print_bundle_help() {
    println!("  bundle create <out.itb> <bundle.toml>              Bundle the modules a manifest lists");
    println!("  bundle create <out.itb> --node <node> [--with-data] Bundle a stopped node as it is");
    println!("  bundle create <out.itb> --example <name>           Bundle an example");
    println!("                  [--name <n>] [--version <v>] override the manifest");
    println!("  bundle inspect <file>                              Show a bundle's manifest and modules");
    println!("  bundle run <file> [--name n] [--port p] [--allow-data-loss|--delete-data]");
    println!("                                 Install the bundle as a node, or update that node, and start it");
    println!("  bundle exe <file.itb> <out>                        Make a program that runs the bundle");
}

async fn create(args: &[String]) -> Result<(), IntersticeError> {
    let out = PathBuf::from(&args[0]);
    let mut bundle = if let Some(node_ref) = flag_value(args, "--node") {
        from_node(&node_ref, has_flag(args, "--with-data")).await?
    } else if let Some(example) = flag_value(args, "--example") {
        example_bundle(&example)?
    } else {
        from_manifest(Path::new(&args[1]))?
    };
    if let Some(name) = flag_value(args, "--name") {
        bundle.manifest.name = name;
    }
    if let Some(version) = flag_value(args, "--version") {
        bundle.manifest.version = version;
    }
    check_modules(&mut bundle)?;
    let bytes = bundle.encode()?;
    std::fs::write(&out, &bytes).map_err(|e| err(format!("{}: {e}", out.display())))?;
    println!(
        "Wrote {} ({}, {} modules)",
        out.display(),
        human(bytes.len()),
        bundle.modules.len()
    );
    Ok(())
}

fn from_manifest(path: &Path) -> Result<Bundle, IntersticeError> {
    let text = std::fs::read_to_string(path).map_err(|e| err(format!("{}: {e}", path.display())))?;
    let manifest: ManifestFile =
        toml::from_str(&text).map_err(|e| err(format!("{}: {e}", path.display())))?;
    let base = path.parent().unwrap_or(Path::new("."));
    let mut modules = Vec::new();
    for entry in &manifest.modules {
        let wasm = if let Some(name) = entry.strip_prefix("builtin:") {
            builtin_module(name)
                .ok_or_else(|| err(format!("unknown builtin module '{name}' (known: {BUILTIN_NAMES})")))?
                .to_vec()
        } else {
            let p = base.join(entry);
            if p.is_dir() {
                build_module_project(&p)?
            } else {
                std::fs::read(&p).map_err(|e| err(format!("{}: {e}", p.display())))?
            }
        };
        modules.push(BundleModule { name: None, wasm, data: Vec::new() });
    }
    Ok(Bundle {
        manifest: BundleManifest {
            name: manifest.name,
            version: manifest.version,
            description: manifest.description,
            port: manifest.port,
            abi_version: ABI_VERSION,
        },
        modules,
    })
}

async fn from_node(node_ref: &str, with_data: bool) -> Result<Bundle, IntersticeError> {
    let registry = NodeRegistry::load()?;
    let record = registry.get(node_ref).ok_or_else(|| err(format!("Unknown node '{node_ref}'")))?;
    let node_id = record
        .node_id
        .clone()
        .filter(|_| record.local)
        .ok_or_else(|| err(format!("'{node_ref}' is not a local node")))?;
    if is_running(&record.address).await {
        return Err(err(format!(
            "Node '{}' is running; stop it first (its files are read directly)",
            record.name
        )));
    }
    let node_dir = nodes_dir().join(node_id);
    let previous = bundle::read_record(&node_dir);
    Ok(Bundle {
        manifest: BundleManifest {
            name: record.name.clone(),
            version: previous.map(|r| r.version).unwrap_or_default(),
            description: String::new(),
            port: record.address.rsplit(':').next().and_then(|p| p.parse().ok()),
            abi_version: ABI_VERSION,
        },
        modules: bundle::modules_from_node(&node_dir, with_data)?,
    })
}

/// Describe every module: fill in names, check the ABI, and check that every
/// module dependency is in the bundle.
fn check_modules(bundle: &mut Bundle) -> Result<(), IntersticeError> {
    let mut schemas = Vec::new();
    for module in &mut bundle.modules {
        let schema = bundle::describe_wasm(&module.wasm)?;
        if schema.abi_version != ABI_VERSION {
            return Err(err(format!(
                "module '{}' was built for ABI {}, this interstice runs ABI {ABI_VERSION}; rebuild it",
                schema.name, schema.abi_version
            )));
        }
        if let Some(name) = &module.name {
            if *name != schema.name {
                return Err(err(format!("module directory '{name}' holds module '{}'", schema.name)));
            }
        }
        module.name = Some(schema.name.clone());
        schemas.push(schema);
    }
    let names: Vec<&str> = schemas.iter().map(|s| s.name.as_str()).collect();
    for (i, name) in names.iter().enumerate() {
        if names[..i].contains(name) {
            return Err(err(format!("module '{name}' is in the bundle twice")));
        }
    }
    for schema in &schemas {
        for dep in &schema.module_dependencies {
            if !names.contains(&dep.module_name.as_str()) {
                return Err(err(format!(
                    "module '{}' depends on '{}', which is not in the bundle",
                    schema.name, dep.module_name
                )));
            }
        }
    }
    Ok(())
}

fn inspect(path: &Path) -> Result<(), IntersticeError> {
    let bundle = Bundle::read(path)?;
    let m = &bundle.manifest;
    println!("Bundle '{}' {}", m.name, m.version);
    if !m.description.is_empty() {
        println!("  {}", m.description);
    }
    println!(
        "  ABI {}{}, port {}",
        m.abi_version,
        if m.abi_version == ABI_VERSION { "" } else { " (this interstice runs another ABI)" },
        m.port.map(|p| p.to_string()).unwrap_or_else(|| "any".into())
    );
    for module in &bundle.modules {
        let data: usize = module.data.iter().map(|f| f.bytes.len()).sum();
        println!(
            "  {:<20} wasm {:>9}{}",
            module.name.as_deref().unwrap_or("?"),
            human(module.wasm.len()),
            if module.data.is_empty() {
                String::new()
            } else {
                format!(", data {} in {} files", human(data), module.data.len())
            }
        );
    }
    Ok(())
}

fn exe(bundle_path: &Path, out: &Path) -> Result<(), IntersticeError> {
    let bundle = Bundle::read(bundle_path)?;
    let me = std::env::current_exe().map_err(|e| err(e.to_string()))?;
    bundle::write_exe(&me, &bundle, out)?;
    let size = std::fs::metadata(out).map(|m| m.len() as usize).unwrap_or(0);
    println!("Wrote {} ({}): running it runs '{}'", out.display(), human(size), bundle.manifest.name);
    Ok(())
}

pub struct RunOptions {
    pub name: Option<String>,
    pub port: Option<u32>,
    pub decision: Option<MigrationPolicy>,
}

impl RunOptions {
    pub fn from_args(args: &[String]) -> Result<Self, IntersticeError> {
        let port = match flag_value(args, "--port") {
            Some(p) => Some(p.parse().map_err(|_| err(format!("bad port '{p}'")))?),
            None => None,
        };
        let decision = if has_flag(args, "--delete-data") {
            Some(MigrationPolicy::DeleteData)
        } else if has_flag(args, "--allow-data-loss") {
            Some(MigrationPolicy::AllowDataLoss)
        } else {
            None
        };
        Ok(Self { name: flag_value(args, "--name"), port, decision })
    }
}

/// Install `bundle` as a node, or update the node of that name, and run it.
pub async fn run_bundle(bundle: Bundle, options: RunOptions) -> Result<(), IntersticeError> {
    if bundle.manifest.abi_version != ABI_VERSION {
        return Err(err(format!(
            "bundle '{}' was built for ABI {}, this interstice runs ABI {ABI_VERSION}",
            bundle.manifest.name, bundle.manifest.abi_version
        )));
    }
    let mut registry = NodeRegistry::load()?;
    let name = options.name.clone().unwrap_or_else(|| bundle.manifest.name.clone());

    if options.decision == Some(MigrationPolicy::DeleteData) && registry.get(&name).is_some() {
        if let Some(record) = registry.get(&name) {
            if is_running(&record.address).await {
                return Err(err(format!("Node '{name}' is running; stop it first")));
            }
        }
        println!("Deleting node '{name}' and its data, then installing fresh.");
        node_utils::remove_node_with_data(&mut registry, &name)?;
    }

    let node = match registry.get(&name).cloned() {
        Some(record) => {
            if !record.local {
                return Err(err(format!("'{name}' is a remote node; bundles install on local nodes")));
            }
            if is_running(&record.address).await {
                return Err(err(format!("Node '{name}' is already running")));
            }
            let id: NodeId = record
                .node_id
                .as_deref()
                .and_then(|id| id.parse().ok())
                .ok_or_else(|| err(format!("Node '{name}' has no id")))?;
            let port: u32 = record
                .address
                .rsplit(':')
                .next()
                .and_then(|p| p.parse().ok())
                .ok_or_else(|| err(format!("Node '{name}' has no port")))?;
            if options.port.is_some_and(|p| p != port) {
                println!("Node '{name}' keeps its port {port}.");
            }
            let installed = bundle::read_record(&nodes_dir().join(id.to_string()));
            match installed {
                Some(r) if r.name == bundle.manifest.name => println!(
                    "Updating '{name}': {} -> {}",
                    or_dash(&r.version),
                    or_dash(&bundle.manifest.version)
                ),
                _ => println!("Applying bundle '{}' to node '{name}'", bundle.manifest.name),
            }
            Node::load(&nodes_dir(), id, port, record.address.clone()).await?
        }
        None => {
            let port = match options.port.or(bundle.manifest.port) {
                Some(port) => port,
                None => free_port()?,
            };
            let address = format!("127.0.0.1:{port}");
            let node = Node::new(&nodes_dir(), port, address.clone())?;
            registry.add(NodeRecord {
                name: name.clone(),
                address: address.clone(),
                node_id: Some(node.id.to_string()),
                local: true,
                last_seen: None,
            })?;
            println!("Installing '{name}' {} on {address}", or_dash(&bundle.manifest.version));
            node
        }
    };

    crate::node_utils::own_node(&node)?;
    let preset = options.decision;
    let decide: Decide = Box::new(move |module: &str, plan: &str| {
        println!("Module '{module}' needs a decision:\n{plan}");
        match preset.or_else(ask_decision) {
            Some(policy) => policy,
            None => {
                println!("Keeping the old version of '{module}'.");
                MigrationPolicy::Safe
            }
        }
    });
    node.start_bundle(bundle, Some(decide)).await
}

fn or_dash(s: &str) -> &str {
    if s.is_empty() { "-" } else { s }
}

async fn is_running(address: &str) -> bool {
    matches!(
        tokio::time::timeout(
            std::time::Duration::from_millis(500),
            tokio::net::TcpStream::connect(address)
        )
        .await,
        Ok(Ok(_))
    )
}

fn free_port() -> Result<u32, IntersticeError> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| err(e.to_string()))?;
    Ok(listener.local_addr().map_err(|e| err(e.to_string()))?.port() as u32)
}

fn human(bytes: usize) -> String {
    match bytes {
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1 << 20) as f64),
        b if b >= 1 << 10 => format!("{:.1} KB", b as f64 / (1 << 10) as f64),
        b => format!("{b} B"),
    }
}
