// Interstice CLI - Command-line interface for schema inspection, validation, and debugging

use interstice_cli::{
    diff_schemas, dry_run_module_load, format_output, OutputFormat, ValidationResult,
};
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_help();
        return;
    }

    let command = &args[1];

    match command.as_str() {
        "schema" if args.len() >= 3 => {
            let path = &args[2];
            let format = if args.len() >= 4 {
                match OutputFormat::from_str(&args[3]) {
                    Ok(fmt) => fmt,
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        return;
                    }
                }
            } else {
                OutputFormat::Text
            };

            // For now, just do a dry-run which validates the schema
            match dry_run_module_load(Path::new(path)) {
                Ok(result) => match format_output(&result, format) {
                    Ok(output) => println!("{}", output),
                    Err(e) => eprintln!("Error: {}", e),
                },
                Err(e) => eprintln!("Error: {}", e),
            }
        }
        "schema-diff" if args.len() >= 4 => {
            let old_path = &args[2];
            let new_path = &args[3];
            let format = if args.len() >= 5 {
                match OutputFormat::from_str(&args[4]) {
                    Ok(fmt) => fmt,
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        return;
                    }
                }
            } else {
                OutputFormat::Text
            };

            match diff_schemas(Path::new(old_path), Path::new(new_path)) {
                Ok(result) => match format_output(&result, format) {
                    Ok(output) => println!("{}", output),
                    Err(e) => eprintln!("Error: {}", e),
                },
                Err(e) => eprintln!("Error: {}", e),
            }
        }
        "validate" if args.len() >= 3 => {
            let path = &args[2];
            let format = if args.len() >= 4 {
                match OutputFormat::from_str(&args[3]) {
                    Ok(fmt) => fmt,
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        return;
                    }
                }
            } else {
                OutputFormat::Text
            };

            match dry_run_module_load(Path::new(path)) {
                Ok(result) => {
                    let validation = ValidationResult {
                        is_valid: result.is_loadable && result.schema_errors.is_empty(),
                        issues: result.schema_errors.clone(),
                        warnings: result.warnings.clone(),
                    };

                    match format_output(&validation, format) {
                        Ok(output) => println!("{}", output),
                        Err(e) => eprintln!("Error: {}", e),
                    }
                }
                Err(e) => eprintln!("Error: {}", e),
            }
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
    println!("  schema <PATH> [FORMAT]           Inspect module schema");
    println!("  schema-diff <OLD> <NEW> [FORMAT] Compare two schemas for compatibility");
    println!("  validate <PATH> [FORMAT]         Validate module schema");
    println!("  log-inspect <PATH> [FORMAT]      Inspect transaction log");
    println!("  help                             Show this help message");
    println!();
    println!("OPTIONS:");
    println!("  FORMAT: json, yaml, text (default: text)");
}
