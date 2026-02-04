// Interstice CLI - Command-line interface

use interstice_core::{interstice_abi::IntersticeValue, IntersticeError, Node};
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
            node.clear_logs().expect("Couldn't clear logs");
            let hello_path = "../../target/wasm32-unknown-unknown/debug/hello.wasm";
            let caller_path = "../../target/wasm32-unknown-unknown/debug/caller.wasm";
            let graphics_path = "../../target/wasm32-unknown-unknown/debug/graphics.wasm";

            let hello_schema = node.load_module(hello_path)?.to_public();
            let caller_schema = node.load_module(caller_path)?.to_public();
            let graphics_schema = node.load_module(graphics_path)?.to_public();

            File::create("./hello_schema.toml")
                .unwrap()
                .write_all(&hello_schema.to_toml_string().unwrap().as_bytes())
                .unwrap();

            let node_schema = node.schema("MyNode".into()).to_public();
            File::create("./node_schema.toml")
                .unwrap()
                .write_all(&node_schema.to_toml_string().unwrap().as_bytes())
                .unwrap();

            node.start().await?;
            node.run(
                "hello",
                "hello",
                IntersticeValue::Vec(vec![IntersticeValue::String("Naloween !".to_string())]),
            )?;
            node.run("caller", "caller", IntersticeValue::Vec(vec![]))?;
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
    println!("  help                             Show this help message");
    println!();
    println!("OPTIONS:");
    println!("  FORMAT: json, yaml, text (default: text)");
}
