use std::{io, path::Path};

use interstice_core::IntersticeError;

// Init module projectect structure in current directory, it should ask the project name create the directory
// and create a template module in it. The template has the basic rust structure with the additional interstice_sdk dependency, a build.rs file filled twith the provided macro
// and a src/lib.rs file with a template module using the interstice_module macro and a simple reducer.
// It should also add a .cargo/config.toml to set the target to be wasm32-unknown-unknown and a Cargo.toml with the interstice_sdk dependency and the build script.
// The command should fail if the current directory is not empty to avoid overwriting files.
pub fn init() -> Result<(), IntersticeError> {
    println!("Enter the project name: ");
    let project_name = &mut String::new();
    io::stdin()
        .read_line(project_name)
        .expect("Should be able to read line");
    let project_name = project_name
        .parse::<String>()
        .map_err(|_| IntersticeError::Internal("Failed to read project name".into()))?
        .trim()
        .to_string();
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
interstice-sdk = "0.3.0"

[build-dependencies]
interstice-sdk = "0.3.0"
"#,
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
#[reducer(on = "init")]
pub fn init(ctx: ReducerContext) {
    ctx.log("Hello world !");
}

#[reducer]
pub fn hello(ctx: ReducerContext, name: String) {
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
fn get_greetings(ctx: QueryContext) -> Vec<Greetings> {
    ctx.current.tables.greetings().scan().unwrap_or_else(|err| {
        ctx.log(&format!("Failed to scan greetings: {}", err));
        vec![]
    })
}
"#;

    std::fs::write(project_path.join("src/lib.rs"), lib_content).map_err(|err| {
        IntersticeError::Internal(format!("Failed to create src/lib.rs: {}", err))
    })?;
    Ok(())
}
