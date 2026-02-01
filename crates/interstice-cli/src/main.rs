// Interstice CLI - Command-line interface

use interstice_core::Node;
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_help();
        return;
    }

    let command = &args[1];

    match command.as_str() {
        "start" => {
            let mut node = match Node::new(Path::new("./transactions_log")) {
                Ok(node) => node,
                Err(err) => panic!("Error when creating interstice node: {}", err),
            };
            node.clear_logs().expect("Couldn't clear logs");
            match node.start() {
                Ok(_) => {}
                Err(err) => panic!("Error when starting interstice node: {}", err),
            };
        }
        "help" | "-h" | "--help" => print_help(),
        _ => {
            eprintln!("Unknown command: {}", command);
            print_help();
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
