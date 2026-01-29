# Interstice – Feature Roadmap & TODOs

This document lists the core features required to make Interstice stable, ergonomic,
and long-lived, before moving to authority and advanced optimizations.

---

- [x] Remove Row types from reducers and convert to the actual table struct
- [ ] add authority host calls (input events, graphics, files, network ? (maybe through subscriptions and requests like spacetimeDB instead))
- [ ] sdk change into to try into for better error management (instead of panic)
- [ ] Add table feature to not be logged (saved)
- [ ] subscription to another node (networking).
- [ ] Add async and parallelization
- [ ] transaction logs snaptchots, separate logs before snapchot (archive) and afetr the current snaptchot
- [ ] transaction logs add indexes to retreive efficiently per module, per table transactions
- [ ] Indexed tables
- [ ] Efficient table scans (avoid full cloning)
- [ ] Columnar / structured storage backend
- [ ] Table migration support
- [ ] Interstice Type Enum support
- [ ] Table Views (allow row filtering based on current state and requesting node id)
- [ ] Subscription execution ordering guarantees ?
- [ ] Modules dependencies & Version compatibility checks
- [ ] Structured logging

## Debugging & Observability

- [ ] Reducer execution tracing
- [ ] Subscription trace graphs
- [ ] Deterministic replay debugging

## Tooling & CLI

- [ ] Schema inspection CLI
- [ ] Module validation CLI
- [ ] Transaction log inspection
- [ ] Replay / determinism checker
- [ ] Dev-mode tracing & logging
- [ ] Hot-reload modules (optional)
