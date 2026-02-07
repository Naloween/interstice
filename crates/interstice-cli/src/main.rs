// Interstice CLI - Command-line interface

use interstice_cli::{
    example::example,
    init::init,
    module::{publish, remove},
    start::{start, start_new},
};
use interstice_core::IntersticeError;

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
            if args.len() < 3 {
                print_help();
                return Ok(());
            } else if args.len() == 3 {
                let port = args[2].trim().parse().expect("Failed to parse port");
                start_new(port).await
            } else if args.len() == 4 {
                let id = args[2].trim().parse().expect("Failed to parse node ID");
                let port = args[3].trim().parse().expect("Failed to parse port");
                start(id, port).await
            } else {
                print_help();
                Ok(())
            }
        }
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
            let node_address = args[2].clone();
            let module_project_path = std::path::Path::new(&args[3]);
            publish(node_address, module_project_path).await
        }
        "remove" => {
            if args.len() < 4 {
                print_help();
                return Ok(());
            }
            let node_address = args[2].clone();
            let module_name = &args[3];
            remove(node_address, module_name).await
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
    println!("  start [port]                           Start an interstice node");
    println!("  start [id] [port]                     Start an interstice node");
    println!(
        "  example [port]                        Start an interstice node example, when on port 8080 it loads the hello module, otherwise it loads the caller and graphics modules"
    );
    println!(
        "  init                                   Initialize a new interstice module project in the current directory"
    );
    println!("  publish <node_address> <module_path>   Publish a module to a node");
    println!("  remove <node_address> <module_name>    Remove a module from a node");
    println!("  help                             Show this help message");
    println!();
    println!("OPTIONS:");
    println!("  No options available for now");
}
