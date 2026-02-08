# Copilot instructions for Interstice

## Big picture (runtime + modules)
- The trusted core lives in crates/interstice-core: `Node` wires networking, runtime, and the window/app loop; see [crates/interstice-core/src/node.rs](crates/interstice-core/src/node.rs) and [crates/interstice-core/src/app.rs](crates/interstice-core/src/app.rs).
- The runtime executes WASM modules via Wasmtime, dispatches events, applies subscriptions, and records transactions; see [crates/interstice-core/src/runtime/mod.rs](crates/interstice-core/src/runtime/mod.rs).
- Modules are WASM components with typed tables + reducers defined via macros in the SDK; examples in [modules/hello/src/lib.rs](modules/hello/src/lib.rs) and [modules/graphics/src/lib.rs](modules/graphics/src/lib.rs).

## Execution flow to keep in mind
- Reducers are scheduled from `EventInstance` (init/render/input/network) and subscriptions; runtime matches and invokes reducers deterministically (see [crates/interstice-core/src/runtime/mod.rs](crates/interstice-core/src/runtime/mod.rs)).
- Node startup loads `.wasm` from the node’s data directory modules folder and replays transaction logs to rebuild state (see [crates/interstice-core/src/node.rs](crates/interstice-core/src/node.rs)).

## Module authoring patterns (SDK)
- Always declare `interstice_module!(...)` at top-level; see public module in [modules/hello/src/lib.rs](modules/hello/src/lib.rs) and authority requests in [modules/graphics/src/lib.rs](modules/graphics/src/lib.rs).
- Data is owned by tables (`#[table]`, `#[primary_key]`) and mutated only inside reducers (`#[reducer]`); example insert and subscription: [modules/hello/src/lib.rs](modules/hello/src/lib.rs).
- Cross-module / cross-node calls use generated bindings; example remote call and subscription name in [modules/caller/src/lib.rs](modules/caller/src/lib.rs).

## Bindings & schemas (important for interop)
- `build.rs` runs `interstice_sdk::bindings::generate_bindings()` to generate `interstice_bindings.rs` from `src/bindings/*.toml`; see [modules/caller/build.rs](modules/caller/build.rs) and schema input [modules/caller/src/bindings/node_schema.toml](modules/caller/src/bindings/node_schema.toml).
- The generator logic lives in [crates/interstice-sdk/src/bindings.rs](crates/interstice-sdk/src/bindings.rs); update TOML schemas when adding reducers/tables for other modules/nodes.

## CLI workflows (developer loop)
- Start a node: `cargo run -p interstice-cli -- start <port>`; CLI entry in [crates/interstice-cli/src/main.rs](crates/interstice-cli/src/main.rs).
- Build WASM modules from workspace root: `cargo build -p <module> --target wasm32-unknown-unknown --release` (see examples in README).
- `interstice-cli init` scaffolds a new module project (template matches hello module); see [crates/interstice-cli/src/init.rs](crates/interstice-cli/src/init.rs).

## Integration points
- Host capabilities (GPU/Input) are gated by authorities in `interstice_module!` and used through `ReducerContext` host calls; see [modules/graphics/src/lib.rs](modules/graphics/src/lib.rs).
- Networked reducer calls and subscriptions are routed through the runtime event loop (see [crates/interstice-core/src/runtime/mod.rs](crates/interstice-core/src/runtime/mod.rs)).
