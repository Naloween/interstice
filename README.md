# Interstice

Interstice is a minimal, modular substrate for running sandboxed WebAssembly modules that cooperate through typed, versioned data and deterministic reducers.

Contents

- Architecture Overview
- Quickstart
- Module authoring
- Examples
- Publishing
- Roadmap & TODOs
- Contribution & License

Repository layout

- The core runtime: [crates/interstice-core](crates/interstice-core)
- The WASM ABI and types: [crates/interstice-abi](crates/interstice-abi)
- The Rust SDK and macros: [crates/interstice-sdk\*](crates/interstice-sdk)
- The CLI: [crates/interstice-cli](crates/interstice-cli)
- Example modules: [modules/hello](modules/hello), [modules/caller](modules/caller), [modules/graphics](modules/graphics), [modules/audio](modules/audio)

---

# Install CLI

Prebuilt binaries are published on GitHub Releases.

Linux / macOS:

```bash
VERSION="0.3.0"
TARGET="x86_64-unknown-linux-gnu" # or x86_64-apple-darwin, aarch64-apple-darwin
curl -L -o interstice.tar.gz \
  https://github.com/Naloween/interstice/releases/download/v${VERSION}/interstice-${VERSION}-${TARGET}.tar.gz
tar -xzf interstice.tar.gz
sudo mv interstice /usr/local/bin/
interstice --help
```

Windows (PowerShell):

```powershell
$Version = "0.3.0"
$Target = "x86_64-pc-windows-msvc"
$Url = "https://github.com/Naloween/interstice/releases/download/v${Version}/interstice-${Version}-${Target}.zip"

Invoke-WebRequest $Url -OutFile interstice.zip
Expand-Archive interstice.zip -DestinationPath .
Move-Item .\interstice.exe "$Env:USERPROFILE\AppData\Local\Microsoft\WindowsApps\"
interstice --help
```

If `interstice` is not found, ensure the destination folder is on your PATH.

From crates.io:

```bash
cargo install interstice-cli
```

---

# Quickstart

Prerequisites

- Rust toolchain (stable) and `cargo`
- Add the WASM target:

```bash
rustup target add wasm32-unknown-unknown
```

Build example modules (from workspace root):

```bash
cargo build -p hello
cargo build -p caller
cargo build -p graphics
cargo build -p audio
```

Start the hello example:

```bash
cargo run -p interstice-cli example hello
```

Start the caller example to simulate remote interactions:

```bash
cargo run -p interstice-cli example caller
```

---

# Module authoring

## Quickstart

The CLI provides a simple init command to start a Rust module for Interstice. It fills the project with a hello example, sets up the config to build for WASM, and adds the required macro calls in build.rs and at the top of lib.rs.

```bash
interstice init
```

## Minimal layout

- `Cargo.toml` — set `crate-type = ["cdylib"]` and depend on `interstice-sdk`.
- `build.rs` — optional helper to produce the WASM artifact.
- `src/lib.rs` — module implementation.

## SDK macros & patterns

At the top of `lib.rs`, call `interstice_module!()` to define the required global WASM glue. The module name is read from Cargo.toml.
You can also pass parameters like `interstice_module!(visibility: Public, authorities: [Gpu, Input])`. `visibility` controls whether the module is accessible from other nodes (default is `Private`). When a module is `Private`, only local modules on the same node can access its reducers, queries, and tables for subscriptions.
The `authorities` argument declares which capabilities the module claims. See below for details.

### Table

Define tables with the `#[table]` macro on top of a struct:

```rust
#[table]
struct MyTable{
  #[primary_key(auto_inc)]
  id: u64,

  #[index(hash, unique)]
  email: String,

  #[index(btree)]
  created_at: i64,

  content: String
}
```

Rules:

- `#[primary_key]` is required and enforces uniqueness. Add `auto_inc` to generate values automatically.
- `#[index(hash)]` and `#[index(btree)]` create secondary indexes. Use `unique` to enforce uniqueness, and `auto_inc` to generate integer values on insert.
- `auto_inc` is supported for integer types only (u8, u32, u64, i32, i64).

#### Persistence modes

Tables default to **logged** persistence, meaning every mutation is appended to that table's log so the node can deterministically replay state. You can opt into other behaviors by annotating the table declaration:

```rust
#[table(stateful)]
struct Accounts { /* ... */ }

#[table(ephemeral)]
struct DerivedCache { /* ... */ }
```

- `Logged` (default) – write-ahead logging only. On restart the node rebuilds the table by replaying its log. Use when you want full transaction history.
- `Stateful` – This does not save transaction logs, only the latest table state in a snapshot. Pick this for large tables that must persist across restarts without additional data usage.
- `Ephemeral` – the table never hits disk: no log, no snapshot. Data lives strictly in-memory for the lifetime of the node process, which is ideal for caches or other derived views you can recompute.

Only one persistence keyword may be used per table. If you omit the keyword you get the default logged behavior.

When inserting, the table API returns the inserted row so you can read generated values:

```rust
let row = ctx.current.tables.mytable().insert(MyTable {
  id: 0,
  email: "user@example.com".to_string(),
  created_at: 0,
  content: "hello".to_string(),
})?;
```

### Interstice types

Inside a table struct, a variety of default types are supported. If you need custom types, use `#[interstice_type]` on top of an enum or struct definition:

```rust
#[interstice_type]
pub enum MyCustomEnum {
  A,
  B(String),
  C(MyCustomStruct),
}

#[interstice_type]
pub struct MyCustomStruct {
  value: i64,
}
```

Note that defining a struct as a table also makes it an interstice type and may be used as such.

### Reducer

After defining your data (tables and types), you will likely define reducers and queries. Reducers do not return anything and may update the tables of the current module. Reducers can call other queries and reducers from other modules.

You define them through the `#[reducer]` marker on top of a function:

```rust
#[reducer]
fn my_reducer(ctx: ReducerContext, my_arg1: u32, my_arg2: MyCustomenum){
  ...
}
```

The first argument of a reducer should always be a `ReducerContext`.
Use `ctx.current.<table>().insert(...)` and `ctx.current.<table>().scan()` for table operations.

Reducers can also subscribe to events, in which case they cannot be called externally.
There are different kinds of events, all following the format:
`#[reducer(on = "<event>")]`

where event can be `init`, `<module>.<table>.<table_event>`, `<node>.<module>.<table>.<table_event>`.

Here `<module>` is the module name you want to subscribe to. For the current module, use the module name defined in Cargo.toml.
`<table>` should be the table name you want to subscribe to.
`<table_event>` can be `insert`, `update` or `delete`.
When subscribing to an event, it requires specific arguments for the reducer. For example, an insert event requires a single additional argument of the table type that receives the inserted row.

#### Core subscription events

The runtime also exposes core events through `#[reducer(on = "...")]`:

- `init` → reducer signature: `fn x(ctx: ReducerContext)`
- `load` → reducer signature: `fn x(ctx: ReducerContext)`
- `connect` → reducer signature: `fn x(ctx: ReducerContext, node_id: String)`
- `disconnect` → reducer signature: `fn x(ctx: ReducerContext, node_id: String)`

`node_id` is the UUID string of the peer node that just connected or disconnected.

Example:

```rust
#[reducer(on = "connect")]
fn on_node_connected(_ctx: ReducerContext, node_id: String) {
  host_calls::log(&format!("peer connected: {}", node_id));
}

#[reducer(on = "disconnect")]
fn on_node_disconnected(_ctx: ReducerContext, node_id: String) {
  host_calls::log(&format!("peer disconnected: {}", node_id));
}
```

### Query

Apart from reducers you may also want to define queries. Similar to reducers, they are defined through the `#[query]` marker on top of functions:

```rust
#[query]
fn my_query(ctx: QueryContext, my_arg1: u32, my_arg2: MyCustomenum) -> MyCustomStruct {
  ...
}
```

Contrary to reducers, queries can return values but are read-only and cannot mutate any tables. They can call other queries but cannot call reducers. They also cannot subscribe to events, since they cannot affect the current state.

### Schedule system

The schedule system provides general-purpose time-based reducer invocation. Instead of relying on window render events or external triggers, modules can schedule reducers to run at specific future times. The runtime maintains a schedule queue and dispatches reducers when their scheduled time arrives.

Schedule a reducer to run after a delay:

```rust
#[reducer]
fn start_timer(ctx: ReducerContext) {
    // Schedule my_callback to run in 1000ms
    ctx.schedule("my_callback", 1000);
}

#[reducer]
fn my_callback(ctx: ReducerContext) {
    // This runs 1000ms after start_timer scheduled it
    // Can reschedule itself for periodic behavior
    ctx.schedule("my_callback", 1000);
}
```

The `ctx.schedule(reducer_name, delay_ms)` host call adds an entry to the runtime's schedule queue. When the delay elapses, the runtime invokes the named reducer with no arguments.

### Bindings

Bindings live in `src/bindings/`.

That folder can contain TOML files describing either:

- Module dependencies (local): module schemas for other modules available in the _same node_.
- Node dependencies (remote): node schemas that include the node address and the public schemas of the modules you depend on.

With only those files, the SDK reads the schemas and generates typed functions to call reducers/queries and subscribe to tables.

When adding a binding, the CLI should fetch the schema from a running node and write it into `src/bindings/`. The schema used is the **public** view (`schema.to_public()`), which strips private tables and (for node schemas) private modules.

## Build for WASM

```bash
rustup target add wasm32-unknown-unknown
cargo build -p <module> --target wasm32-unknown-unknown --release
```

You can omit the target argument if the .cargo/config.toml is already well configured, which is the case when you used the init cli command.

---

# Examples

- `modules/hello`
  - `Greetings` table, `hello` reducer, and an `init` hook.
- `modules/caller`
  - Uses generated bindings to call `hello` remotely and subscribes to `hello.greetings.insert`.
- `modules/graphics`
  - Requests `Input` and `Gpu` authorities and renders a triangle; implements `init`, `render`, and `input` hooks.
- `modules/audio`
  - Requests `Audio` authority and plays a tone while logging input buffers.

Reproduce

1. Start the hello example (`interstice example hello`).
2. Start the caller example (`interstice example caller`) so it can call hello.
3. Start graphics or audio with `interstice example graphics` or `interstice example audio`.

---

# Publishing

## Manual workflow

1. Build module WASM:

```bash
cargo build -p hello --target wasm32-unknown-unknown --release
```

2. Locate the WASM in `target/wasm32-unknown-unknown/release/`.
3. Use this path to manually add a module from rust code using `Node::load_module()`

There is no manual way to publish to an already running node. See the CLI flow below.

## CLI flow

- `interstice publish <node> <module-rust-project-path>` — build, upload, validate, and install a module on a running node. The node verifies schema compatibility and requested capabilities.

# CLI usage

## Data layout

- CLI metadata lives under the OS data directory (`data_file()` in the CLI).
- Node registry is stored in `nodes.toml` (friendly names, addresses, IDs, etc.).
- Node runtime data lives under `nodes/<node_id>/` (modules, logs, transaction log).

## Node management

- `interstice node add <name> <address>`
- `interstice node create <name> <port>`
- `interstice node list`
- `interstice node remove <name|id>`
- `interstice node rename <old> <new>`
- `interstice node show <name|id>`
- `interstice node start <name|id>`
- `interstice node ping <name|id>`
- `interstice node schema <name|id> [out]`

## Example command

- `interstice example <hello|caller|graphics|audio>`
- Built-in ports are fixed by example name: `hello=8080`, `caller=8081`, `graphics=8082`, `audio=8083`.

## Bindings helpers

- `interstice bindings add module <node> <module> [project_path]`
- `interstice bindings add node <node> [project_path]`

These commands fetch **public** schemas from the target node and write TOML files into `src/bindings/`.

## Module commands

- `interstice publish <node> <module_path>`
- `interstice remove <node> <module_name>`
- `interstice call_reducer <node> <module_name> <reducer_name> [args...]`
- `interstice call_query <node> <module_name> <query_name> [args...]`
- `interstice update`

# Security

- Publishing doesn't require any privilege by default, so anyone can publish and remove modules, even remotely.
- To prevent this default behavior, the node should load a module with the Module authority. In this case, all requests are forwarded to this module, which can enforce custom policies for publish/remove and access.

---

# Architecture Overview

Interstice is organized around a small trusted core that loads, sandboxes, and executes WASM modules. Modules express functionality entirely through typed, versioned interfaces composed of tables and reducers. The core is responsible for state, scheduling, capability enforcement, and deterministic ordering; modules own logic and optional privileged abilities when granted.

## Key concepts

- Node: a runtime process hosting modules and exposing endpoints for inter-node calls.
- Module: a WASM component with a serialized interface (tables, reducers, requested authorities, version).
- Table: typed, versioned records owned by a module; mutations happen inside reducers.
- Reducer: deterministic state-transition function that runs inside a module.
- Query: deterministic read-only function that runs inside a module and returns a value.
- Subscription: declarative binding that schedules reducers when events occur (table changes, initialization, input event...).

## Authorities

Authorities are typed tokens granting modules access to privileged host functionality (gpu access, input event...). Only one module can hold an authority at a time. Declare them via `interstice_module!(authorities: [...])` so the runtime can enforce exclusivity.

- **Gpu** – grants access to the render loop plus GPU host calls. Modules with this authority can receive `render` events and submit draw commands to the host surface (see `modules/graphics`).
- **Audio** – allows the module to stream audio samples or capture input through host calls. Reducers can subscribe to `audio_output` and `audio_input` events for output ticks and input readiness.
- **Input** – subscribes the module to keyboard/mouse/controller events and lets it inspect the current input state through the `input` reducer.
- **File** – provides controlled access to the node's data directory for reading assets, watching paths, or performing limited file IO needed for development workflows.
- **Module** – designates a module as the module-manager for that node. When present, all publish/remove requests are routed through it (see the Security section) so it can enforce custom policies.

## Execution model

1. Reducer invocation (external call or subscription)
2. Reducer performs host calls to mutate tables
3. Core records changes and resolves subscriptions
4. Dependent reducers are scheduled deterministically

## Determinism and concurrency

- Deterministic replay is a design goal: given the same inputs, module versions, and initial state, execution is reproducible.
- Reducers are currently processed through a serialized queue in the runtime event loop.

---

# Roadmap & TODOs

This roadmap is a living checklist of the main directions for Interstice. It favors clarity over fixed timelines and can evolve as the runtime grows.

---

## Security and data access

- Add module caller name to context
- Table views and row-level security: allow modules to filter rows based on runtime state and requesting node id
- Time travel host call: should beable to time travel some table, creating timelines and branches (reason: very cool and allow easy time-related effects in games and apps in general). There should be several kind of travels changing the behavior of branching, what is saved and what not etc...

## Runtime and data model

- Schedule system
- Table migrations and schema evolution without data loss
- Default system modules (ModuleManager, Graphics, Inputs)
- Audio authority and host calls

## Robustness and correctness

- Clean runtime, node and engines code (app, network, audio, file)
- Rename the Input authority to be more xplicit (audio also has input subscription)
- Improve macro checks and error messages (subscription args and types)
- Harden network reconnections and peer health handling
- Expand function-level documentation across core and SDK

## Performance and determinism

- Iter-based table scans and more efficient index access
- Reduce IntersticeValue conversions and avoid unnecessary clones
- Parallelize reducers when safe under deterministic constraints

## Tooling, diagnostics, and DX

- Benchmarks, profiling tools, and performance budgets
- Time travel tooling: rewind and inspect previous node/module states

---

# License

This repository is licensed under the MIT License. See `LICENSE` for details.
