// Interstice CLI - Command-line interface

use interstice_cli::{
    call_query::call_query,
    call_reducer::call_reducer,
    example::example,
    init::init,
    module::{publish, remove},
    start::start,
    bindings::{add_module_binding, add_node_binding},
    data_directory::nodes_dir,
    node_client::handshake_with_node,
    node_registry::{NodeRecord, NodeRegistry},
};
use interstice_core::{IntersticeError, Node, interstice_abi::IntersticeValue};
use std::path::Path;

#[tokio::main]
async fn main() -> Result<(), IntersticeError> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_help();
        return Ok(());
    }

    let command = &args[1];

    match command.as_str() {
        "node" => handle_node_command(&args).await,
        "bindings" => handle_bindings_command(&args).await,
        "example" => {
            if args.len() < 3 {
                print_help();
                return Ok(());
            }
            let port = args[2].trim().parse().expect("Failed to parse port");
            example(port).await
        }
        "init" => init(),
        "publish" => {
            if args.len() < 4 {
                print_help();
                return Ok(());
            }
            let node_ref = args[2].clone();
            let module_project_path = std::path::Path::new(&args[3]);
            publish(node_ref, module_project_path).await
        }
        "remove" => {
            if args.len() < 4 {
                print_help();
                return Ok(());
            }
            let node_ref = args[2].clone();
            let module_name = &args[3];
            remove(node_ref, module_name).await
        }
        "call_reducer" => {
            if args.len() < 5 {
                print_help();
                return Ok(());
            }
            let node_ref = args[2].clone();
            let module_name = args[3].clone();
            let reducer_name = args[4].clone();
            let mut input_vec: Vec<IntersticeValue> = Vec::new();
            for arg in &args[5..] {
                input_vec.push(arg.clone().into());
            }
            let input = IntersticeValue::Vec(input_vec);
            call_reducer(node_ref, module_name, reducer_name, input.into()).await
        }
        "call_query" => {
            if args.len() < 5 {
                print_help();
                return Ok(());
            }
            let node_ref = args[2].clone();
            let module_name = args[3].clone();
            let query_name = args[4].clone();
            let mut input_vec: Vec<IntersticeValue> = Vec::new();
            for arg in &args[5..] {
                input_vec.push(arg.clone().into());
            }
            let input = IntersticeValue::Vec(input_vec);
            call_query(node_ref, module_name, query_name, input.into()).await
        }
        "help" | "-h" | "--help" => {
            print_help();
            Ok(())
        }
        _ => {
            eprintln!("Unknown command: {}", command);
            print_help();
            Ok(())
        }
    }
}

fn print_help() {
    println!("Interstice CLI - Module and transaction management");
    println!();
    println!("USAGE:");
    println!("  interstice <COMMAND> [OPTIONS]");
    println!();
    println!("COMMANDS:");
    println!("  node add <name> <address>            Add a node entry");
    println!("  node create <name> <port> [--elusive]  Create a local node entry");
    println!("  node list                            List known nodes");
    println!("  node remove <name|id>                Remove a node entry");
    println!("  node rename <old> <new>              Rename a node entry");
    println!("  node show <name|id>                  Show node details");
    println!("  node start <name|id>                 Start a local node by name");
    println!("  node ping <name|id>                  Check node connectivity");
    println!("  node schema <name|id> [out]          Fetch node schema");
    println!("  bindings add module <node> <module> [project_path]  Add module binding");
    println!("  bindings add node <node> [project_path]            Add node binding");
    println!(
        "  example [port]                        Start an interstice node example, when on port 8080 it loads the hello module, otherwise it loads the caller and graphics modules"
    );
    println!(
        "  init                                   Initialize a new interstice module project in the current directory"
    );
    println!("  publish <node> <module_path>   Publish a module to a node");
    println!("  remove <node> <module_name>    Remove a module from a node");
    println!(
        "  call_reducer <node> <module_name> <reducer_name>    Call a reducer of a module on a node"
    );
    println!(
        "  call_query <node> <module_name> <query_name>        Call a query of a module on a node"
    );
    println!("  help                             Show this help message");
    println!();
    println!("OPTIONS:");
    println!("  No options available for now");
}

async fn handle_node_command(args: &[String]) -> Result<(), IntersticeError> {
    if args.len() < 3 {
        print_help();
        return Ok(());
    }

    let mut registry = NodeRegistry::load()?;
    match args[2].as_str() {
        "add" => {
            if args.len() < 5 {
                print_help();
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
                elusive: false,
            })?;
            println!("Node added.");
        }
        "create" => {
            if args.len() < 5 {
                print_help();
                return Ok(());
            }
            let name = args[3].clone();
            let port: u32 = args[4].trim().parse().expect("Failed to parse port");
            let address = format!("127.0.0.1:{}", port);
            if args.iter().any(|arg| arg == "--elusive") {
                registry.add(NodeRecord {
                    name,
                    address,
                    node_id: None,
                    local: true,
                    last_seen: None,
                    elusive: true,
                })?;
                println!("Local node created (elusive).");
            } else {
                let node = Node::new(&nodes_dir(), port)?;
                registry.add(NodeRecord {
                    name,
                    address,
                    node_id: Some(node.id.to_string()),
                    local: true,
                    last_seen: None,
                    elusive: false,
                })?;
                println!("Local node created.");
            }
        }
        "list" => {
            for node in registry.list_sorted() {
                let id = node.node_id.clone().unwrap_or_else(|| "-".into());
                let last_seen = node
                    .last_seen
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| "-".into());
                println!(
                    "{} | {} | {} | {}",
                    node.name, node.address, id, last_seen
                );
            }
        }
        "remove" => {
            if args.len() < 4 {
                print_help();
                return Ok(());
            }
            registry.remove(&args[3])?;
            println!("Node removed.");
        }
        "rename" => {
            if args.len() < 5 {
                print_help();
                return Ok(());
            }
            registry.rename(&args[3], &args[4])?;
            println!("Node renamed.");
        }
        "show" => {
            if args.len() < 4 {
                print_help();
                return Ok(());
            }
            let node = registry
                .get(&args[3])
                .ok_or_else(|| IntersticeError::Internal("Node not found".into()))?;
            println!("name: {}", node.name);
            println!("address: {}", node.address);
            println!("node_id: {}", node.node_id.clone().unwrap_or_else(|| "-".into()));
            println!("local: {}", node.local);
            println!("elusive: {}", node.elusive);
            println!(
                "last_seen: {}",
                node.last_seen.map(|t| t.to_string()).unwrap_or_else(|| "-".into())
            );
        }
        "start" => {
            if args.len() < 4 {
                print_help();
                return Ok(());
            }
            let node = registry
                .get(&args[3])
                .ok_or_else(|| IntersticeError::Internal("Node not found".into()))?;
            let port = node
                .address
                .split(':')
                .last()
                .ok_or_else(|| IntersticeError::Internal("Invalid address".into()))?
                .parse()
                .map_err(|_| IntersticeError::Internal("Invalid port".into()))?;
            if node.elusive {
                let node = Node::new_elusive(port)?;
                registry.set_node_id(&args[3], node.id.to_string());
                registry.set_last_seen(&args[3]);
                registry.save()?;
                node.start().await?;
            } else {
                let node_id = node
                    .node_id
                    .clone()
                    .ok_or_else(|| IntersticeError::Internal("Missing node id".into()))?;
                start(node_id.parse().unwrap(), port).await?;
            }
        }
        "ping" => {
            if args.len() < 4 {
                print_help();
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
                print_help();
                return Ok(());
            }
            let node_ref = &args[3];
            let out_path = args.get(4).map(Path::new).unwrap_or_else(|| Path::new("node_schema.toml"));
            let address = registry
                .resolve_address(node_ref)
                .ok_or_else(|| IntersticeError::Internal("Unknown node".into()))?;
            let node_name = registry
                .get(node_ref)
                .map(|n| n.name.clone())
                .unwrap_or_else(|| node_ref.clone());
            let (schema, handshake) = interstice_cli::node_client::fetch_node_schema(&address, &node_name).await?;
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
        _ => print_help(),
    }
    Ok(())
}

async fn handle_bindings_command(args: &[String]) -> Result<(), IntersticeError> {
    if args.len() < 4 {
        print_help();
        return Ok(());
    }
    match args[2].as_str() {
        "add" => match args[3].as_str() {
            "module" => {
                if args.len() < 6 {
                    print_help();
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
                    print_help();
                    return Ok(());
                }
                let node_ref = &args[4];
                let project_path = args.get(5).map(Path::new).unwrap_or_else(|| Path::new("."));
                let out_path = add_node_binding(node_ref, project_path).await?;
                println!("Binding written to {}", out_path.display());
            }
            _ => print_help(),
        },
        _ => print_help(),
    }
    Ok(())
}

