use crate::{node_client::fetch_node_schema, node_registry::NodeRegistry};
use interstice_core::IntersticeError;
use std::path::{Path, PathBuf};

pub async fn handle_bindings_command(args: &[String]) -> Result<(), IntersticeError> {
    if args.len() < 4 {
        print_bindings_help();
        return Ok(());
    }

    match args[2].as_str() {
        "add" => match args[3].as_str() {
            "module" => {
                if args.len() < 6 {
                    print_bindings_help();
                    return Ok(());
                }
                let node_ref = &args[4];
                let module_name = &args[5];
                let project_path = args.get(6).map(Path::new).unwrap_or_else(|| Path::new("."));
                let out_path = add_module_binding(node_ref, module_name, project_path).await?;
                println!("Binding written to {}", out_path.display());
            }
            "node" => {
                if args.len() < 5 {
                    print_bindings_help();
                    return Ok(());
                }
                let node_ref = &args[4];
                let project_path = args.get(5).map(Path::new).unwrap_or_else(|| Path::new("."));
                let out_path = add_node_binding(node_ref, project_path).await?;
                println!("Binding written to {}", out_path.display());
            }
            _ => print_bindings_help(),
        },
        _ => print_bindings_help(),
    }

    Ok(())
}

fn print_bindings_help() {
    println!("USAGE:");
    println!("  interstice bindings add module <node> <module> [project_path]");
    println!("  interstice bindings add node <node> [project_path]");
}

pub async fn add_module_binding(
    node_ref: &str,
    module_name: &str,
    project_path: &Path,
) -> Result<PathBuf, IntersticeError> {
    let mut registry = NodeRegistry::load()?;
    let address = registry
        .resolve_address(node_ref)
        .ok_or_else(|| IntersticeError::Internal(format!("Unknown node '{}'", node_ref)))?;
    let node_name = registry
        .get(node_ref)
        .map(|n| n.name.clone())
        .unwrap_or_else(|| node_ref.to_string());
    let (schema, handshake) = fetch_node_schema(&address, &node_name).await?;
    registry.set_last_seen(node_ref);
    registry.set_node_id(node_ref, handshake.node_id);
    registry.save()?;

    let module = schema
        .modules
        .into_iter()
        .find(|m| m.name == module_name)
        .ok_or_else(|| {
            IntersticeError::Internal(format!("Module '{}' not found in node schema", module_name))
        })?;

    let bindings_dir = project_path.join("src").join("bindings");
    std::fs::create_dir_all(&bindings_dir).map_err(|err| {
        IntersticeError::Internal(format!("Failed to create bindings directory: {err}"))
    })?;
    let out_path = bindings_dir.join(format!("{}.toml", module_name));
    let contents = module.to_public().to_toml_string().map_err(|err| {
        IntersticeError::Internal(format!("Failed to serialize module schema: {err}"))
    })?;
    std::fs::write(&out_path, contents).map_err(|err| {
        IntersticeError::Internal(format!("Failed to write module binding: {err}"))
    })?;
    Ok(out_path)
}

pub async fn add_node_binding(
    node_ref: &str,
    project_path: &Path,
) -> Result<PathBuf, IntersticeError> {
    let mut registry = NodeRegistry::load()?;
    let address = registry
        .resolve_address(node_ref)
        .ok_or_else(|| IntersticeError::Internal(format!("Unknown node '{}'", node_ref)))?;
    let node_name = registry
        .get(node_ref)
        .map(|n| n.name.clone())
        .unwrap_or_else(|| node_ref.to_string());
    let (schema, handshake) = fetch_node_schema(&address, &node_name).await?;
    registry.set_last_seen(node_ref);
    registry.set_node_id(node_ref, handshake.node_id);
    registry.save()?;

    let bindings_dir = project_path.join("src").join("bindings");
    std::fs::create_dir_all(&bindings_dir).map_err(|err| {
        IntersticeError::Internal(format!("Failed to create bindings directory: {err}"))
    })?;
    let file_name = sanitize_filename(&node_name);
    let out_path = bindings_dir.join(format!("node_{}.toml", file_name));
    let contents = schema.to_public().to_toml_string().map_err(|err| {
        IntersticeError::Internal(format!("Failed to serialize node schema: {err}"))
    })?;
    std::fs::write(&out_path, contents)
        .map_err(|err| IntersticeError::Internal(format!("Failed to write node binding: {err}")))?;
    Ok(out_path)
}

fn sanitize_filename(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}
