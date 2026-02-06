// Interstice CLI - Command-line interface

use interstice_core::{IntersticeError, Node};
use std::{fs::File, io::Write as _, path::Path};

#[tokio::main]
async fn main() -> Result<(), IntersticeError> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_help();
        return Ok(());
    }

    let command = &args[1];

    match command.as_str() {
        "start" => {
            let port = args[2].trim().parse().expect("Failed to parse port");
            let mut node = Node::new(Path::new("./transactions_log"), port)?;
            node.clear_logs().await.expect("Couldn't clear logs");
            node.start().await?;
            Ok(())
        }
        "example" => {
            let port = args[2].trim().parse().expect("Failed to parse port");
            let mut node = Node::new(Path::new("./transactions_log"), port)?;
            node.clear_logs().await.expect("Couldn't clear logs");
            let hello_path = "../../target/wasm32-unknown-unknown/debug/hello.wasm";
            let caller_path = "../../target/wasm32-unknown-unknown/debug/caller.wasm";
            let graphics_path = "../../target/wasm32-unknown-unknown/debug/graphics.wasm";

            if port != 8080 {
                // Client
                let _caller_schema = node.load_module(caller_path).await?.to_public();
                let _graphics_schema = node.load_module(graphics_path).await?.to_public();
            } else {
                // Server
                let hello_schema = node.load_module(hello_path).await?; //.to_public();
                File::create("./hello_schema.toml")
                    .unwrap()
                    .write_all(&hello_schema.to_toml_string().unwrap().as_bytes())
                    .unwrap();
            }

            let node_schema = node.schema("MyNode".into()).await.to_public();
            File::create("./node_schema.toml")
                .unwrap()
                .write_all(&node_schema.to_toml_string().unwrap().as_bytes())
                .unwrap();

            node.start().await?;
            Ok(())
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
    println!("  start                            Start an interstice node");
    println!("  example                          Start an interstice node example, when on port 8080 it loads the hello module, otherwise it loads the caller and graphics modules");
    println!("  help                             Show this help message");
    println!();
    println!("OPTIONS:");
    println!("  No options available for now");
}
