// Interstice CLI - Command-line interface

use interstice_cli::{
    benchmark::handle_benchmark_command,
    bindings::handle_bindings_command,
    call_query::call_query,
    call_reducer::call_reducer,
    example::example,
    init::init,
    module::{publish, remove},
    node_utils::handle_node_command,
    update::update,
};
use interstice_core::{IntersticeError, interstice_abi::IntersticeValue};
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
            example(&args[2]).await
        }
        "init" => init(),
        "publish" => {
            if args.len() < 4 {
                print_help();
                return Ok(());
            }
            let node_ref = args[2].clone();
            let module_project_path = Path::new(&args[3]);
            publish(node_ref, module_project_path).await
        }
        "update" => update(),
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
        "benchmark" => handle_benchmark_command(&args).await,
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
        "  example <hello|caller|graphics|audio|agar-server|agar-client|benchmark>  Start a named example (ports: hello=8080, caller=8081, graphics=8082, audio=8083, agar-server=8086, agar-client=8084, benchmark=8085)"
    );
    println!(
        "  init                                   Initialize a new interstice module project in the current directory"
    );
    println!("  publish <node> <module_path>   Publish a module to a node");
    println!("  remove <node> <module_name>    Remove a module from a node");
    println!("  update                          Update the interstice CLI");
    println!(
        "  call_reducer <node> <module_name> <reducer_name>    Call a reducer of a module on a node"
    );
    println!(
        "  call_query <node> <module_name> <query_name>        Call a query of a module on a node"
    );
    println!("  benchmark <...>                    Run benchmark workloads and scenarios");
    println!("  help                             Show this help message");
    println!();
    println!("OPTIONS:");
    println!("  No options available for now");
}
