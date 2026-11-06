//! The default modules are embedded (`src/example.rs`); they are built by
//! `scripts/build_modules.sh`, not kept in git (DESIGN §10).

const DEFAULTS: [&str; 5] = ["graphics", "audio", "input", "module_manager", "network"];

fn main() {
    println!("cargo:rerun-if-changed=module_defaults");
    let missing: Vec<&str> = DEFAULTS
        .iter()
        .copied()
        .filter(|m| !std::path::Path::new(&format!("module_defaults/{m}.wasm")).exists())
        .collect();
    if !missing.is_empty() {
        panic!(
            "\n\nThe CLI embeds the default modules, and these are not built: {}.\n\
             Run `scripts/build_modules.sh --defaults` from the repository root \
             (it needs `rustup target add wasm32-unknown-unknown`).\n\n",
            missing.join(", ")
        );
    }
}
