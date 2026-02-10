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
- Example modules: [modules/hello], [modules/caller], [modules/graphics]

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
```

Go to the cli crate:

```bash
cd crates/interstice-cli
```

Start a node (port 8080):

```bash
cargo run example 8080
```

Start a second node (port 8081) to simulate remote interactions:

```bash
cargo run example 8081
```

---

# Module authoring

## Quickstart

the CLI provides an easy simple init commands to start a rust module for interstice, it fills the project with a simple hello example, setup the config to build for wasm, adds the needed macros calls in build.rs and at the top of lib.rs

```bash
interstice-cli run init
```

## Minimal layout

- `Cargo.toml` — set `crate-type = ["cdylib"]` and depend on `interstice-sdk`.
- `build.rs` — optional helper to produce the WASM artifact.
- `src/lib.rs` — module implementation.

## SDK macros & patterns

At the top of `lib.rs`, you need to call `interstice_module!()` to define the required global wasm glue for interstice. The name of the module is taken from the Cargo.toml.
Additionaly you can specify two parameters `interstice_module!(visibility: Public, authorities: [Gpu, Input])`. The `visibility` tells if the module is accessible from other nodes (default to `Private`). This means that if the moduel is `Private`, only local modules from the same node can access the module's reducers, queries and tables for subscription.
the `authorities` argument define potential authority claimed by this module. See below for further information.

### Table

Inside your module you may define tables through the `#[table]` macro on top of a struct:
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

When inserting, the table API returns the inserted row so you can read generated values:
```rust
let row = ctx.current.tables.mytable().insert(MyTable {
  id: 0,
  email: "user@example.com".to_string(),
  created_at: 0,
  content: "hello".to_string(),
})?;
```

### Interstice Type

In a table struct, a variety of default types are supported as field. However if you need fields with your own types you may use `#[interstice_type]` on top of enum or struct definition:

```rust
#[interstice_type]
pub MyCustomEnum {
  A,
  B(String),
  C(MyCustomStruct),
}

#[interstice_type]
pub MyCustomStruct {
  value: i64,
}
```

Note that defining a struct as a table also makes it an interstice type and may be used as such.

### Reducer

After defining your data (tables and types) you probably want to define some reducers and queries. Reducers don't return anything and may update the tables of the current module. Reducers can call other queries and reducers from other modules.

You define them through the `#[reducer]` marker on top of a function:

```rust
#[reducer]
fn my_reducer(ctx: ReducerContext, my_arg1: u32, my_arg2: MyCustomenum){
  ...
}
```

The first argument of a reducer should always be a `ReducerContext`.
Use `ctx.current.<table>().insert(...)` and `ctx.current.<table>().scan()` for table operations.

Additionally reducers can subscribe to a particular event, in which case they cannot be called externally in another way.
There is different kind of events, all abide by the format:
`#[reducer(on = "<event>")]`

where event can be `init`, `<module>.<table>.<table_event>`, `<node>.<module>.<table>.<table_event>`.

Here `<module>` is the module name where you want to subscribe to, if current module you should put the current module name defined in Cargo.toml.
`<table>` should be table name you want to subscribe to.
`<table_event>` can be `insert`, `update` or `delete`.
When subscribing to an event, it imposes specific arguments for the reducer. For example the insert event impose to have only one additional argument of type of the table where you subscribed and will be the inserted row.

### Query

Appart from reducers you may also want to define queries. Similarly to reducers thay are defined through `#[query]` marker on top of functions:

```rust
#[query]
fn my_query(ctx: QueryContext, my_arg1: u32, my_arg2: MyCustomenum) -> MyCustomStruct {
  ...
}
```

Constrary to reducers, queries can return some value but are read only and cannot mutate any tables. They can call other queries but cannot call other reducers. they also cannot subscribe to any event as they cannot have any effect on the current state.

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

Reproduce

1. Start two nodes (ports 8080 and 8081).
2. Build and install `hello` on one node and `caller` on the other.
3. Run `graphics` on a node to see the triangle.

---

# Publishing

## Manual workflow

1. Build module WASM:

```bash
cargo build -p hello --target wasm32-unknown-unknown --release
```

2. Locate the WASM in `target/wasm32-unknown-unknown/release/`.
3. Use this path to manually add a module from rust code using `Node::load_module()`

There is no way of publishing to an already started node manually. See the CLI flow below.

## CLI flow

- `interstice-cli publish <node-address> <module-rust-project-path>` — build, upload, validate, and install a module on a running node. The node verifies schema compatibility and requested capabilities.

# Security

- Publishing doesn't require any priviledge by default, so anyone can publish and remove module, even remotely.
- To prevent this default behavior the node needs to have loaded a module with the Module authority. In this case, all request will be forwarded to this module. This is the only module capable of publishing and removing module on the node it runs. 

---

# Architecture Overview

Interstice is organized around a small trusted core that loads, sandboxes, and executes WASM modules. Modules express functionality entirely through typed, versioned interfaces composed of tables and reducers. The core is responsible for state, scheduling, capability enforcement, and deterministic ordering; modules own logic and optional privileged abilities when granted.

## Key concepts

- Node: a runtime process hosting modules and exposing endpoints for inter-node calls.
- Module: a WASM component with a serialized interface (tables, reducers, requested authorities, version).
- Table: typed, versioned records owned by a module; mutations happen inside reducers.
- Reducer: deterministic state-transition function that run inside a module.
- Query: deterministic read-only function that run inside a module and return some value
- Subscription: declarative binding that schedule reducers when events occur (table changes, initialization, input event...).

## Authorities

Authorities are typed tokens granting modules access to privileged host functionality (gpu access, input event...). Only one module can hold an authority at a time.

## Execution model

1. Reducer invocation (external call or subscription)
2. Reducer performs host calls to mutate tables
3. Core records changes and resolves subscriptions
4. Dependent reducers are scheduled deterministically

## Determinism and concurrency

- Deterministic replay is a design goal: given the same inputs, module versions, and initial state, execution is reproducible.
- The core may parallelize execution when it can prove no conflicting writes will occur.

---

# Roadmap & TODOs

This document lists the core features required to make Interstice stable, ergonomic and long-lived, before moving to advanced optimizations.

---

## Features

- Table Views (allow row filtering based on current state and requesting node id)
- Add token to confirm node identities on connection (generate one token per node connecting to one one)
- add audio authority
- Table migration support
- Subscription execution ordering guarantees ?
- Add elusive table feature (to not be logged (saved)). Usefull for non persistent states like the mouse position.

## Robustness, error handling and fixes

- Change the macro building to use quote instead of raw strings
- Network handle reconnections and be more robust
- Gpu error handling instead of panic (frame not begun etc.. Especially on resize where it interrupts the current render)
- macros more checks and better error handlings (subscription check args and types)

## Optimizations

- Efficient table scans through iter
- Better type convertions designs (instead of always converting to IntersticeValue as an intermediate)
- Optimize type convertions (no clones)
- transaction logs snaptchots, separate logs before snapchot (archive) and after the current snaptchot
- transaction logs add indexes to retreive efficiently per module, per table transactions
- Columnar / structured storage backend
- parallelize reducers calls when possible

## Tooling & CLI

- Make the CLI instantiate a node with default modules to manage all the commands, connect to other modules and so on (this also shows that we can have whole programs embeded in a module seemlessly)
- Update interstice
- Benchmarkings
- Rewind time and monitor previous module states and node states

---

# License

This repository is licensed under the MIT License. See `LICENSE` for details.
