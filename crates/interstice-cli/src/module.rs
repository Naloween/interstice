use crate::node_client::connect;
use crate::node_registry::NodeRegistry;
use interstice_core::{IntersticeError, LoadOutcome, MigrationPolicy};
use std::io::{BufRead, IsTerminal, Write};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub async fn load(
    node_ref: String,
    module_project_path: &Path,
    decision: Option<MigrationPolicy>,
    mut grant: bool,
) -> Result<(), IntersticeError> {
    // This should take a path to a rust project that it will build and load. The module name is from Cargo.toml. It should build the project using cargo, then read the generated wasm file and send it to the node using the network module.
    // It should also be able to use saved servers nodes with their adress to easily load to known nodes.

    // connect to node
    let registry = NodeRegistry::load()?;
    let node_address = registry
        .resolve_address(&node_ref)
        .ok_or_else(|| IntersticeError::Internal("Unknown node".into()))?;
    let client = connect(&node_address).await?;

    let wasm_binary = build_module_project(module_project_path)?;

    // Load with the safe policy first; if the migration plan needs a
    // decision, ask (or use the flag) and send again (DESIGN §3.3).
    // `--delete-data` always wipes, like SpacetimeDB's; `--allow-data-loss`
    // only answers a plan that asks.
    let mut policy = match decision {
        Some(MigrationPolicy::DeleteData) => MigrationPolicy::DeleteData,
        _ => MigrationPolicy::Safe,
    };
    let result = loop {
        let outcome = client.load_module(wasm_binary.clone(), policy, grant).await?;
        match outcome {
            LoadOutcome::Loaded { plan } => {
                if !plan.is_empty() {
                    println!("{plan}");
                }
                println!("Module loaded.");
                break Ok(());
            }
            LoadOutcome::Forwarded => {
                println!("The node's module authority handles loads; the request was forwarded to it.");
                break Ok(());
            }
            LoadOutcome::Failed(err) => break Err(IntersticeError::Internal(format!("Load failed: {err}"))),
            LoadOutcome::NeedsGrant { module, authorities } => {
                // DESIGN §7.4: the requester grants authorities.
                if grant {
                    break Err(IntersticeError::Internal("The node still asks for a grant".into()));
                }
                if ask_grant(&module, &authorities) {
                    grant = true;
                } else {
                    println!("Not granted: nothing changed on the node.");
                    break Ok(());
                }
            }
            LoadOutcome::NeedsDecision { plan } => {
                println!("{plan}");
                if policy != MigrationPolicy::Safe {
                    break Err(IntersticeError::Internal("The node still asks for a decision".into()));
                }
                match decision.or_else(ask_decision) {
                    Some(choice) => policy = choice,
                    None => {
                        println!("Aborted: nothing changed on the node.");
                        break Ok(());
                    }
                }
            }
        }
    };

    client.close().await;
    result
}

/// Build the module project at `module_project_path` with cargo and return
/// its wasm.
pub(crate) fn build_module_project(module_project_path: &Path) -> Result<Vec<u8>, IntersticeError> {
    // Build module using cargo
    println!(
        "Building module from {} using cargo...",
        module_project_path.display()
    );
    let manifest_path = module_project_path.join("Cargo.toml");
    let output = std::process::Command::new("cargo")
        .args([
            "build",
            "--release",
            "--target",
            "wasm32-unknown-unknown",
            "--manifest-path",
            manifest_path.to_string_lossy().as_ref(),
        ])
        .output()
        .expect("Failed to execute cargo build command");
    if !output.status.success() {
        return Err(IntersticeError::Internal(format!(
            "Cargo build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    // Read generated wasm file
    let wasm_path = resolve_wasm_path(&manifest_path)?;
    std::fs::read(&wasm_path).map_err(|e| {
        IntersticeError::Internal(format!("Failed to read {}: {e}", wasm_path.display()))
    })

}

/// Ask whether to grant `authorities` to `module`; no terminal means no.
fn ask_grant(module: &str, authorities: &[String]) -> bool {
    println!("Module '{module}' asks for authorities: {}", authorities.join(", "));
    if !std::io::stdin().is_terminal() {
        println!("No terminal to ask: pass --grant to grant them.");
        return false;
    }
    print!("Grant {} to '{module}'? [y/N] ", authorities.join(", "));
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    if std::io::stdin().lock().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_lowercase().as_str(), "y" | "yes")
}

/// Ask how to proceed with a plan that needs a decision; `None` aborts.
/// Without a terminal it aborts.
pub(crate) fn ask_decision() -> Option<MigrationPolicy> {
    if !std::io::stdin().is_terminal() {
        println!("No terminal to ask: pass --allow-data-loss or --delete-data to decide.");
        return None;
    }
    loop {
        print!("[a]bort / [m]igrate (apply as listed) / [d]elete this module's data? [a] ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        if std::io::stdin().lock().read_line(&mut line).is_err() {
            return None;
        }
        match line.trim().to_lowercase().as_str() {
            "" | "a" | "abort" => return None,
            "m" | "migrate" => return Some(MigrationPolicy::AllowDataLoss),
            "d" | "delete" => return Some(MigrationPolicy::DeleteData),
            _ => println!("Please answer a, m or d."),
        }
    }
}

pub async fn unload(node_ref: String, module_name: &str) -> Result<(), IntersticeError> {
    // Unload a module from the node without deleting its persisted data, so it
    // can be loaded again later and resume with its tables intact.

    // connect to node
    let registry = NodeRegistry::load()?;
    let node_address = registry
        .resolve_address(&node_ref)
        .ok_or_else(|| IntersticeError::Internal("Unknown node".into()))?;
    let client = connect(&node_address).await?;
    client.unload_module(module_name)?;
    client.close().await;

    Ok(())
}

pub async fn remove(node_ref: String, module_name: &str) -> Result<(), IntersticeError> {
    // This should take a module name and send a message to the node to delete the module with that name. It should also be able to use saved servers nodes with their adress to easily delete from known nodes.

    // connect to node
    let registry = NodeRegistry::load()?;
    let node_address = registry
        .resolve_address(&node_ref)
        .ok_or_else(|| IntersticeError::Internal("Unknown node".into()))?;
    let client = connect(&node_address).await?;
    client.remove_module(module_name)?;
    client.close().await;

    Ok(())
}

fn resolve_wasm_path(manifest_path: &Path) -> Result<PathBuf, IntersticeError> {
    let output = std::process::Command::new("cargo")
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
            manifest_path.to_string_lossy().as_ref(),
        ])
        .output()
        .map_err(|err| {
            IntersticeError::Internal(format!("Failed to run cargo metadata: {}", err))
        })?;

    let metadata: Value = serde_json::from_slice(&output.stdout).map_err(|err| {
        IntersticeError::Internal(format!("Failed to parse cargo metadata: {}", err))
    })?;

    let target_directory = metadata
        .get("target_directory")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IntersticeError::Internal("Missing target_directory".into()))?;

    let packages = metadata
        .get("packages")
        .and_then(|v| v.as_array())
        .ok_or_else(|| IntersticeError::Internal("Missing packages".into()))?;

    let manifest_str = normalize_path(manifest_path)?;

    let package = packages
        .iter()
        .find(|pkg| {
            pkg.get("manifest_path")
                .and_then(|v| v.as_str())
                .and_then(|p| normalize_path_str(p).ok())
                .map(|p| p == manifest_str)
                .unwrap_or(false)
        })
        .ok_or_else(|| IntersticeError::Internal("Could not find package in metadata".into()))?;

    let package_name = package
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IntersticeError::Internal("Missing package name".into()))?;

    // Cargo converts hyphens to underscores in output filenames
    let wasm_filename = package_name.replace('-', "_");

    Ok(PathBuf::from(target_directory)
        .join("wasm32-unknown-unknown")
        .join("release")
        .join(wasm_filename)
        .with_extension("wasm"))
}

fn normalize_path(path: &Path) -> Result<String, IntersticeError> {
    let canonical = path
        .canonicalize()
        .map_err(|err| IntersticeError::Internal(format!("Invalid manifest path: {}", err)))?;
    Ok(normalize_path_buf(canonical))
}

fn normalize_path_str(path: &str) -> Result<String, IntersticeError> {
    Ok(normalize_path_buf(PathBuf::from(path)))
}

fn normalize_path_buf(path: PathBuf) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    s.strip_prefix("//?/")
        .or_else(|| s.strip_prefix("\\\\?\\"))
        .unwrap_or(&s)
        .to_string()
}
