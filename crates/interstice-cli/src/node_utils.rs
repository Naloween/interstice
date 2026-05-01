use crate::{
    data_directory::nodes_dir,
    node_client::{fetch_node_schema, handshake_with_node},
    node_registry::{NodeRecord, NodeRegistry},
    start::start,
};
use interstice_core::{IntersticeError, Node};
use std::path::Path;

/// Remove a node from the registry and clean up its local data directory if it's a local node
pub fn remove_node_with_data(
    registry: &mut NodeRegistry,
    name_or_id: &str,
) -> Result<(), IntersticeError> {
    let removed = registry.remove(name_or_id)?;
    if removed.local {
        if let Some(node_id) = removed.node_id {
            let node_path = nodes_dir().join(node_id);
            if node_path.exists() {
                std::fs::remove_dir_all(&node_path).map_err(|err| {
                    IntersticeError::Internal(format!(
                        "Failed to remove node data at {}. \
                        Is another instance still running? Error: {err}",
                        node_path.display()
                    ))
                })?;
            }
        }
    }
    Ok(())
}

pub async fn handle_node_command(args: &[String]) -> Result<(), IntersticeError> {
    if args.len() < 3 {
        print_node_help();
        return Ok(());
    }

    let mut registry = NodeRegistry::load()?;
    match args[2].as_str() {
        "add" => {
            if args.len() < 5 {
                print_node_help();
                return Ok(());
            }
            let name = args[3].clone();
            let address = args[4].clone();
            registry.add(NodeRecord {
                name,
                address,
                node_id: None,
                local: false,
                last_seen: None,
            })?;
            println!("Node added.");
        }
        "create" => {
            if args.len() < 5 {
                print_node_help();
                return Ok(());
            }
            let name = args[3].clone();
            let port: u32 = args[4]
                .trim()
                .parse()
                .map_err(|err| IntersticeError::Internal(format!("Failed to parse port: {err}")))?;
            let public_address = format!("127.0.0.1:{}", port);
            let node = Node::new(&nodes_dir(), port, public_address.clone())?;
            registry.add(NodeRecord {
                name,
                address: public_address,
                node_id: Some(node.id.to_string()),
                local: true,
                last_seen: None,
            })?;
            println!("Local node created.");
        }
        "list" => {
            for node in registry.list_sorted() {
                let id = node.node_id.clone().unwrap_or_else(|| "-".into());
                let last_seen = node
                    .last_seen
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| "-".into());
                println!("{} | {} | {} | {}", node.name, node.address, id, last_seen);
            }
        }
        "remove" => {
            if args.len() < 4 {
                print_node_help();
                return Ok(());
            }
            remove_node_with_data(&mut registry, &args[3])?;
            println!("Node removed.");
        }
        "rename" => {
            if args.len() < 5 {
                print_node_help();
                return Ok(());
            }
            registry.rename(&args[3], &args[4])?;
            println!("Node renamed.");
        }
        "show" => {
            if args.len() < 4 {
                print_node_help();
                return Ok(());
            }
            let node = registry
                .get(&args[3])
                .ok_or_else(|| IntersticeError::Internal("Node not found".into()))?;
            println!("name: {}", node.name);
            println!("address: {}", node.address);
            println!(
                "node_id: {}",
                node.node_id.clone().unwrap_or_else(|| "-".into())
            );
            println!("local: {}", node.local);
            println!(
                "last_seen: {}",
                node.last_seen
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| "-".into())
            );
        }
        "start" => {
            if args.len() < 4 {
                print_node_help();
                return Ok(());
            }
            let node = registry
                .get(&args[3])
                .ok_or_else(|| IntersticeError::Internal("Node not found".into()))?;
            let port = node
                .address
                .split(':')
                .next_back()
                .ok_or_else(|| IntersticeError::Internal("Invalid address".into()))?
                .parse()
                .map_err(|_| IntersticeError::Internal("Invalid port".into()))?;

            let node_id = node
                .node_id
                .clone()
                .ok_or_else(|| IntersticeError::Internal("Missing node id".into()))?;
            let parsed_node_id = node_id
                .parse()
                .map_err(|_| IntersticeError::Internal("Invalid node id".into()))?;
            start(parsed_node_id, port, node.address.clone()).await?;
        }
        "ping" => {
            if args.len() < 4 {
                print_node_help();
                return Ok(());
            }
            let address = registry
                .resolve_address(&args[3])
                .ok_or_else(|| IntersticeError::Internal("Unknown node".into()))?;
            let (_stream, handshake) = handshake_with_node(&address).await?;
            registry.set_last_seen(&args[3]);
            registry.set_node_id(&args[3], handshake.node_id);
            registry.save()?;
            println!("Node reachable.");
        }
        "schema" => {
            if args.len() < 4 {
                print_node_help();
                return Ok(());
            }
            let node_ref = &args[3];
            let out_path = args
                .get(4)
                .map(Path::new)
                .unwrap_or_else(|| Path::new("node_schema.toml"));
            let address = registry
                .resolve_address(node_ref)
                .ok_or_else(|| IntersticeError::Internal("Unknown node".into()))?;
            let node_name = registry
                .get(node_ref)
                .map(|node| node.name.clone())
                .unwrap_or_else(|| node_ref.clone());
            let (schema, handshake) = fetch_node_schema(&address, &node_name).await?;
            registry.set_last_seen(node_ref);
            registry.set_node_id(node_ref, handshake.node_id);
            registry.save()?;
            let contents = schema.to_toml_string().map_err(|err| {
                IntersticeError::Internal(format!("Failed to serialize schema: {err}"))
            })?;
            std::fs::write(out_path, contents).map_err(|err| {
                IntersticeError::Internal(format!("Failed to write schema: {err}"))
            })?;
            println!("Schema written to {}", out_path.display());
        }
        _ => print_node_help(),
    }

    Ok(())
}

fn print_node_help() {
    println!("USAGE:");
    println!("  interstice node add <name> <address>");
    println!("  interstice node create <name> <port>");
    println!("  interstice node list");
    println!("  interstice node remove <name|id>");
    println!("  interstice node rename <old> <new>");
    println!("  interstice node show <name|id>");
    println!("  interstice node start <name|id>");
    println!("  interstice node ping <name|id>");
    println!("  interstice node schema <name|id> [out]");
}
