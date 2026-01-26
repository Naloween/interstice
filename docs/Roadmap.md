# Interstice – Feature Roadmap & TODOs

This document lists the core features required to make Interstice stable, ergonomic,
and long-lived, before moving to authority and advanced optimizations.

---

TODOS

- Add persistent runtime across different reducer calls (maybe through network ?)
- add authority host calls (input events, graphics, files, network ? (maybe through subscriptions and requests like spacetimeDB instead))
- Add async and parallelization
- transaction logs snaptchots, separate logs before snapchot (archive) and afetr the current snaptchot
- transaction logs add indexes to retreive efficiently per module, per table transactions
- Add multiple instances (multi-node runtime)

## 1. Core Runtime (mostly done)

- [x] WASM module loading
- [x] Stable ABI + version checking
- [x] Reducers as deterministic entrypoints
- [x] Cross-module reducer calls (sync)
- [x] Table definitions via module schema
- [x] Transactional table mutations
- [x] Subscription system (insert/update/delete)
- [x] Event queue with deterministic ordering
- [x] Reducer cycle detection

---

## 2. Persistence & Durability (CRITICAL)

- [ ] Append-only transaction log
- [ ] Persist committed reducer transactions
- [ ] Replay engine to reconstruct state on startup
- [ ] Skip reducer execution during replay
- [ ] Disable subscriptions during replay
- [ ] Periodic state snapshots (optional but recommended)
- [ ] Log integrity verification
- [ ] Schema compatibility checks during replay

---

## 3. Tables & Storage

- [x] Schema-validated rows
- [x] Primary keys enforced
- [ ] Indexed tables
- [ ] Efficient table scans (avoid full cloning)
- [ ] Columnar / structured storage backend
- [ ] Table migration support
- [ ] Table schema versioning

---

## 4. SDK Ergonomics (High Priority)

- [x] `#[reducer]` macro
- [x] `#[table]` macro
- [x] subscription macro `#[reducer(on = module.table.event)]`
- [ ] Typed table handles (no raw `Row`)
- [ ] Typed reducer calls (no strings)
- [ ] Compile-time reducer signature validation
- [ ] Compile-time table name validation
- [ ] Reducer context (`&Context`) abstraction
- [ ] Read-only vs read-write contexts
- [ ] Eliminate direct `IntersticeValue` usage in user code

---

## 5. Custom Types & Type System

- [ ] `#[derive(IntersticeType)]`
- [ ] Struct support
- [ ] Enum support
- [ ] Nested types
- [ ] Option / Vec support
- [ ] Compile-time rejection of unsupported layouts
- [ ] Automatic schema generation for custom types

---

## 6. Subscriptions (Enhancements)

- [x] Static subscriptions via schema
- [x] Typed event payloads (Insert / Update / Delete)
- [ ] Multiple subscription filtering rules
- [ ] Subscription execution ordering guarantees
- [ ] Subscription isolation (no accidental state mutation)
- [ ] Subscription debugging hooks

---

## 7. Cross-Module Interfaces & Imports

- [ ] Schema import mechanism
- [ ] Version compatibility checks
- [ ] Compile-time reducer interface validation
- [ ] Typed cross-module reducer calls
- [ ] Public / private visibility enforcement
- [ ] Schema hash pinning

---

## 8. Execution Model & Safety

- [ ] Explicit reducer execution phases
- [ ] Explicit commit phase
- [ ] Deterministic scheduling guarantees
- [ ] Maximum subscription depth limits
- [ ] Panic propagation / error surfacing from WASM
- [ ] Runtime error classification

---

## 9. Tooling & CLI

- [ ] Schema inspection CLI
- [ ] Module validation CLI
- [ ] Transaction log inspection
- [ ] Replay / determinism checker
- [ ] Dev-mode tracing & logging
- [ ] Hot-reload modules (optional)

---

## 10. Debugging & Observability

- [ ] Structured logging
- [ ] Reducer execution tracing
- [ ] Transaction visualization
- [ ] Subscription trace graphs
- [ ] Deterministic replay debugging

---

## 11. Performance (Later)

- [ ] Zero-copy ABI paths
- [ ] Batched host calls
- [ ] Indexed table access
- [ ] Parallel read-only reducers
- [ ] Snapshot compression

---

## 12. Authority & Permissions (Future)

- [ ] Auth context in reducers
- [ ] Capability-based permissions
- [ ] Table access control
- [ ] Reducer call authorization
- [ ] Auditable permission changes

---

## Design Invariants (Must Always Hold)

- Reducers are deterministic
- All state changes are transactional
- State is replayable from logs
- Module interfaces are explicit and versioned
- Subscriptions are derived behavior, not state
