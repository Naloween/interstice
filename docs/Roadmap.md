# Interstice – Feature Roadmap & TODOs

This document lists the core features required to make Interstice stable, ergonomic,
and long-lived, before moving to authority and advanced optimizations.

---

- [x] Remove Row types from reducers and convert to the actual table struct
- [x] Interstice Type Enum support
- [x] add authorities management
- [x] add input authority
- [x] add gpu authority
- [x] Modules dependencies & Version compatibility checks
- [x] Load modules after app is initialized
- [ ] Network handle disconnection (remove subscriptions)
- [ ] Gpu error handling instead of panic (frame not begun etc.. Especially on resize where it interrupts the current render)
- [ ] subscription to another node table (networking). So add Node sdk with nodes registry etc...
- [ ] save modules loaded, node id etc.. Along with the table datas.
- [ ] add file authority
- [ ] add audio authority
- [ ] Add async and parallelization
- [ ] Auto_inc flag table column
- [ ] Indexed tables (add index flag macro on field struct)
- [ ] Get table row by index (primary key and indexed columns)
- [ ] macros more checks and better error handlings (subscription check args and types)
- [ ] Efficient table scans through iter
- [ ] Better type convertions designs (instead of always converting to IntersticeValue as an intermediate)
- [ ] Optimize type convertions (no clones)
- [x] sdk change "into" to "try into" for better error management (instead of panic)
- [ ] transaction logs snaptchots, separate logs before snapchot (archive) and afetr the current snaptchot
- [ ] transaction logs add indexes to retreive efficiently per module, per table transactions
- [ ] Columnar / structured storage backend
- [ ] Table migration support
- [ ] Table Views (allow row filtering based on current state and requesting node id)
- [ ] Subscription execution ordering guarantees ?
- [ ] Add table feature to not be logged (saved)
- [ ] Structured logging

## Tooling & CLI

- [x] Start node
- [ ] Init module (build.rs, Cargo.toml, src/lib.rs, .cargo/config.toml for wasm32 build with corresponding default macros)
- [ ] publish module (build to wasm and send the file to the node at the specified adress)
- [ ] Update interstice
- [ ] Transaction log inspection
- [ ] Replay / determinism checker

## Debugging & Observability

- [ ] Reducer execution tracing
- [ ] Subscription trace graphs
- [ ] Deterministic replay debugging
