// Interstice CLI - Command-line interface

use interstice_cli::{
    example::example,
    init::init,
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
    println!("  help                             Show this help message");
    println!();
    println!("OPTIONS:");
    println!("  No options available for now");
}
