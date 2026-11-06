use std::{io, path::Path};

use interstice_core::IntersticeError;

/// `interstice init [name]`: create `<name>/` with a hello-world module:
/// `Cargo.toml` (cdylib, `interstice-sdk`), `build.rs` (bindings),
/// `.cargo/config.toml` (wasm32 target) and `src/lib.rs`. Asks for the name
/// when it is not given; refuses an existing directory.
pub fn init(name: Option<&str>) -> Result<(), IntersticeError> {
    let project_name = match name {
        Some(name) => name.trim().to_string(),
        None => {
            println!("Enter the project name: ");
            let mut line = String::new();
            io::stdin()
                .read_line(&mut line)
                .map_err(|e| IntersticeError::Internal(format!("Failed to read the project name: {e}")))?;
            line.trim().to_string()
        }
    };
    if project_name.is_empty() {
        return Err(IntersticeError::Internal("give a project name: interstice init <name>".into()));
    }
    let project_path = Path::new(&project_name);
    if project_path.exists() {
        return Err(IntersticeError::Internal("Directory already exists".into()));
    }
    std::fs::create_dir(&project_path).map_err(|err| {
        IntersticeError::Internal(format!("Failed to create project directory: {}", err))
    })?;

    let name = project_name.to_lowercase();

    // Create Cargo.toml
    let cargo_toml_content = format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
interstice-sdk = "{sdk}"

[build-dependencies]
interstice-sdk = "{sdk}"
"#,
        // The SDK released with this CLI (they share the workspace version).
        sdk = env!("CARGO_PKG_VERSION"),
    );
    std::fs::write(project_path.join("Cargo.toml"), cargo_toml_content).map_err(|err| {
        IntersticeError::Internal(format!("Failed to create Cargo.toml: {}", err))
    })?;

    // Create .cargo/config.toml
    let cargo_config_content = r#"[build]
target = "wasm32-unknown-unknown"
"#;
    std::fs::create_dir(project_path.join(".cargo")).map_err(|err| {
        IntersticeError::Internal(format!("Failed to create .cargo directory: {}", err))
    })?;
    std::fs::write(
        project_path.join(".cargo/config.toml"),
        cargo_config_content,
    )
    .map_err(|err| {
        IntersticeError::Internal(format!("Failed to create .cargo/config.toml: {}", err))
    })?;

    // Create build.rs
    let build_rs_content = r#"fn main() {
    interstice_sdk::bindings::generate_bindings();
}
"#;
    std::fs::create_dir(project_path.join("src")).map_err(|err| {
        IntersticeError::Internal(format!("Failed to create src directory: {}", err))
    })?;
    std::fs::write(project_path.join("build.rs"), build_rs_content)
        .map_err(|err| IntersticeError::Internal(format!("Failed to create build.rs: {}", err)))?;

    // Create src/lib.rs
    let lib_content = r#"use interstice_sdk::*;

interstice_module!(visibility: Public);

// TABLES

#[table(public)]
#[derive(Debug)]
pub struct Greetings {
    #[primary_key(auto_inc)]
    pub id: u64,
    #[index(btree, unique)]
    pub greeting: String,
    pub custom: TestCustomType,
}

#[interstice_type]
#[derive(Debug, Clone)]
pub struct TestCustomType {
    pub val: u32,
}

// REDUCERS
#[reducer(on = "load")]
pub fn init(ctx: ReducerContext) {
    ctx.log("Hello world !");
}

#[reducer]
pub fn hello<Caps>(ctx: ReducerContext<Caps>, name: String)
where
    Caps: CanInsert<Greetings>,
{
    ctx.log(&format!("Saying hello to {}", name));
    match ctx.current.tables.greetings().insert(Greetings {
        id: 0,
        greeting: format!("Hello, {}!", name),
        custom: TestCustomType { val: 0 },
    }) {
        Ok(_) => (),
        Err(err) => ctx.log(&format!("Failed to insert greeting: {:?}", err)),
    }
}

#[reducer(on = "hello.greetings.insert")]
fn on_greeting_insert(ctx: ReducerContext, inserted_row: Greetings) {
    ctx.log(&format!("Inserted greeting: {:?}", inserted_row));
}

#[query]
fn get_greetings<Caps>(ctx: QueryContext<Caps>) -> Result<Vec<Greetings>, String>
where
    Caps: CanRead<Greetings>,
{
    ctx.current.tables.greetings().scan()
}
"#;

    std::fs::write(project_path.join("src/lib.rs"), lib_content).map_err(|err| {
        IntersticeError::Internal(format!("Failed to create src/lib.rs: {}", err))
    })?;
    Ok(())
}
