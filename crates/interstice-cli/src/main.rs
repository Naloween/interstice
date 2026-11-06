// Interstice CLI - Command-line interface

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use interstice_cli::{
    benchmark::handle_benchmark_command,
    bindings::handle_bindings_command,
    bundle::{RunOptions, handle_bundle_command, print_bundle_help, run_bundle},
    call_query::call_query,
    call_reducer::call_reducer,
    example::example,
    init::init,
    module::{load, remove, unload},
    node_utils::handle_node_command,
    update::update,
};
use interstice_core::{IntersticeError, interstice_abi::IntersticeValue};
use std::path::Path;

#[tokio::main]
async fn main() -> Result<(), IntersticeError> {
    let args: Vec<String> = std::env::args().collect();

    // A program made by `bundle exe` carries its bundle after the binary
    // (DESIGN §9.1): run it instead of the CLI.
    if let Ok(me) = std::env::current_exe() {
        if let Some(bundle) = interstice_core::bundle::read_from_exe(&me)? {
            if args.iter().any(|a| a == "-h" || a == "--help") {
                println!("{} {}", bundle.manifest.name, bundle.manifest.version);
                println!("Runs the node '{}' (an Interstice bundle).", bundle.manifest.name);
                println!("  [--port p]          port for the first install");
                println!("  [--allow-data-loss] apply updates that drop data without asking");
                println!("  [--delete-data]     start fresh: delete the node and its data");
                return Ok(());
            }
            return run_bundle(bundle, RunOptions::from_args(&args[1..])?).await;
        }
    }

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
        "bundle" => handle_bundle_command(&args).await,
        "identity" => {
            let identity = interstice_cli::data_directory::load_cli_identity()?;
            println!("id:    {}", identity.cli_id);
            println!("token: {}", identity.cli_token);
            println!("On another machine: interstice node owner add <node> {} {}", identity.cli_id, identity.cli_token);
            Ok(())
        }
        "init" => init(),
        "load" => {
            if args.len() < 4 {
                print_help();
                return Ok(());
            }
            let node_ref = args[2].clone();
            let module_project_path = Path::new(&args[3]);
            // Answers the migration prompt in advance (DESIGN §3.3).
            let decision = if args[4..].iter().any(|a| a == "--delete-data") {
                Some(interstice_core::MigrationPolicy::DeleteData)
            } else if args[4..].iter().any(|a| a == "--allow-data-loss") {
                Some(interstice_core::MigrationPolicy::AllowDataLoss)
            } else {
                None
            };
            let grant = args[4..].iter().any(|a| a == "--grant");
            load(node_ref, module_project_path, decision, grant).await
        }
        "update" => update(),
        "unload" => {
            if args.len() < 4 {
                print_help();
                return Ok(());
            }
            let node_ref = args[2].clone();
            let module_name = &args[3];
            unload(node_ref, module_name).await
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
    println!("  node create <name> <port> [--expose]  Create a local node (listens on localhost unless exposed)");
    println!("  node expose <name|id> on|off         Listen on every interface, or on localhost only");
    println!("  node owner add|list <name|id> ...    Owners may administer the node and call anything");
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
        "  example <hello|caller|graphics|audio|agar-server|agar-client|ui|notes|desktop|http-get|browser>  Start a named example (ports: hello=8080, caller=8081, graphics=8082, audio=8083, agar-server=8086, agar-client=8084, ui=8088, desktop=8089, http-get=8090, browser=8091, notes=8092)"
    );
    print_bundle_help();
    println!(
        "  init                                   Initialize a new interstice module project in the current directory"
    );
    println!("  load <node> <module_path> [--allow-data-loss|--delete-data] [--grant]");
    println!("                                 Load or update a module; asks before a migration that loses data");
    println!("                                 and before granting authorities (--grant grants them)");
    println!("  identity                       Show this CLI's id and token (to become an owner of a remote node)");
    println!("  unload <node> <module_name>    Unload a module but keep its data");
    println!("  remove <node> <module_name>    Remove a module and delete its data");
    println!("  update                          Update the interstice CLI");
    println!(
        "  call_reducer <node> <module_name> <reducer_name>    Call a reducer of a module on a node"
    );
    println!(
        "  call_query <node> <module_name> <query_name>        Call a query of a module on a node"
    );
    println!("  benchmark simple <...>             Run simple benchmark workload");
    println!("  help                             Show this help message");
    println!();
    println!("OPTIONS:");
    println!("  No options available for now");
}
