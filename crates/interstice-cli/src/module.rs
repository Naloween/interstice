use crate::node_client::handshake_with_node;
use crate::node_registry::NodeRegistry;
use interstice_core::{IntersticeError, ModuleEventInstance, NetworkPacket, packet::write_packet};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub async fn publish(node_ref: String, module_project_path: &Path) -> Result<(), IntersticeError> {
    // This should take a path to a rust project that it will build and publish. The module name is from Cargo.toml. It should build the project using cargo, then read the generated wasm file and send it to the node using the network module.
    // It should also be able to use saved servers nodes with their adress to easily publish to known nodes.

    // connect to node
    let registry = NodeRegistry::load()?;
    let node_address = registry
        .resolve_address(&node_ref)
        .ok_or_else(|| IntersticeError::Internal("Unknown node".into()))?;
    let (mut stream, _handshake) = handshake_with_node(&node_address).await?;

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
    let wasm_binary = std::fs::read(wasm_path).expect("Failed to read generated wasm file");

    // Send wasm binary to node
    let packet = NetworkPacket::ModuleEvent(ModuleEventInstance::Publish { wasm_binary });
    write_packet(&mut stream, &packet).await?;

    // Close connection properly
    let packet = NetworkPacket::Close;
    write_packet(&mut stream, &packet).await?;

    Ok(())
}

pub async fn remove(node_ref: String, module_name: &str) -> Result<(), IntersticeError> {
    // This should take a module name and send a message to the node to delete the module with that name. It should also be able to use saved servers nodes with their adress to easily delete from known nodes.

    // connect to node
    let registry = NodeRegistry::load()?;
    let node_address = registry
        .resolve_address(&node_ref)
        .ok_or_else(|| IntersticeError::Internal("Unknown node".into()))?;
    let (mut stream, _handshake) = handshake_with_node(&node_address).await?;

    // Send wasm binary to node
    let packet = NetworkPacket::ModuleEvent(ModuleEventInstance::Remove {
        module_name: module_name.into(),
    });
    write_packet(&mut stream, &packet).await?;

    // Close connection properly
    let packet = NetworkPacket::Close;
    write_packet(&mut stream, &packet).await?;

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
