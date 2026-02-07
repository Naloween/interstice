# Interstice

Interstice is a minimal, modular substrate for running sandboxed WebAssembly modules that cooperate through typed, versioned data and deterministic reducers.

Contents

- Architecture Overview
- Quickstart
- Module authoring
- Examples
- Publishing (draft)
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

Start a node (port 8080):

```bash
cargo run -p interstice-cli -- start 8080
```

Start a second node (port 8081) to simulate remote interactions:

```bash
cargo run -p interstice-cli -- start 8081
```

Build example modules (from workspace root):

```bash
cargo build -p hello --target wasm32-unknown-unknown --release
cargo build -p caller --target wasm32-unknown-unknown --release
cargo build -p graphics --target wasm32-unknown-unknown --release
```

Loading / publishing modules (current)

Currently the node loads modules from the modules directory or via manual install. A `publish` CLI command is planned (see Publishing section below) to upload and validate module WASM artifacts on a running node.

---

# Module authoring

Minimal layout

- `Cargo.toml` — set `crate-type = ["cdylib"]` and depend on `interstice-sdk`.
- `build.rs` — optional helper to produce the WASM artifact.
- `src/lib.rs` — module implementation.

SDK macros & patterns

- `interstice_module!(...)` — register the module (visibility, authorities).
- `#[table]` — mark table row structs; use `#[primary_key]` on the key field.
- `#[interstice_type]` — custom serializable types.
- `#[reducer]` — declare reducers; `on = "event"` subscribes to events.

Reducer & table usage

Reducers receive `ReducerContext` and typed args. Use `ctx.current.<table>().insert(...)` and `ctx.current.<table>().scan()` for table operations. Keep reducers deterministic and avoid blocking operations; model async work via events.

Capabilities

Declare requested authorities with `interstice_module!(authorities: [Input, Gpu]);` for privileged modules.

Build for WASM

```bash
rustup target add wasm32-unknown-unknown
cargo build -p <module> --target wasm32-unknown-unknown --release
```

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
2. Build and install `hello` on one node and `caller` on the other (manual copy/publish step today).
3. Run `graphics` on a node with display privileges to see the triangle.

---

# Publishing (draft)

Manual workflow

1. Build module WASM:

```bash
cargo build -p hello --target wasm32-unknown-unknown --release
```

2. Locate the WASM in `target/wasm32-unknown-unknown/release/`.
3. Copy the artifact into the node's modules directory or use the planned CLI `publish` command.

Planned CLI flow

- `interstice-cli publish --addr <host:port> <module.wasm>` — upload, validate, and install a module on a running node. The node verifies schema compatibility and requested capabilities.

Security

- Publishing requires operator privileges and authenticated endpoints. The node must validate requested capabilities and allow grant/revoke.

For details see `docs/Publishing.md`.

---

# Architecture Overview

Interstice is organized around a small trusted core that loads, sandboxes, and executes WASM modules. Modules express functionality entirely through typed, versioned interfaces composed of tables and reducers. The core is responsible for state, scheduling, capability enforcement, and deterministic ordering; modules own logic and optional privileged abilities when granted.

Key concepts

- Node: a runtime process hosting modules and exposing endpoints for inter-node calls.
- Module: a WASM component with a serialized interface (tables, reducers, requested authorities, version).
- Table: typed, versioned records owned by a module; mutations happen inside reducers.
- Reducer: deterministic state-transition functions that run inside the module with a `ReducerContext`.
- Subscription: declarative binding that schedule reducers when events occur (table changes, initialization, input event...).

Authorities

Authorities are typed tokens granting modules access to privileged host functionality (gpu access, input event...). Only one module can hold an authority at a time.

Execution model

1. Reducer invocation (external call or subscription)
2. Reducer performs host calls to mutate tables
3. Core records changes and resolves subscriptions
4. Dependent reducers are scheduled deterministically

Determinism and concurrency

- Deterministic replay is a design goal: given the same inputs, module versions, and initial state, execution is reproducible.
- The core may parallelize execution when it can prove no conflicting writes will occur.

---

# Roadmap & TODOs

This document lists the core features required to make Interstice stable, ergonomic,
and long-lived, before moving to authority and advanced optimizations.

---

- Auto_inc flag table column
- Indexed tables (add index flag macro on field struct)
- Get table row by index (primary key and indexed columns)
- Table Views (allow row filtering based on current state and requesting node id)
- Network handle reconnections and be more robust
- Gpu error handling instead of panic (frame not begun etc.. Especially on resize where it interrupts the current render)
- add file authority
- add audio authority
- add module authority (ability to load, delete update modules on the current node)
- parallelize runtime
- macros more checks and better error handlings (subscription check args and types)
- Efficient table scans through iter
- Better type convertions designs (instead of always converting to IntersticeValue as an intermediate)
- Optimize type convertions (no clones)
- transaction logs snaptchots, separate logs before snapchot (archive) and after the current snaptchot
- transaction logs add indexes to retreive efficiently per module, per table transactions
- Columnar / structured storage backend
- Table migration support
- Subscription execution ordering guarantees ?
- Add table feature to not be logged (saved) with the elusive attribute. Usefull for non persistent state like the mouse position.

## Tooling & CLI

- publish module (build to wasm and send the file to the node at the specified adress)
- Update interstice
- Transaction log inspection
- Benchmarkings

---

# License

This repository is licensed under the MIT License. See `LICENSE` for details.
