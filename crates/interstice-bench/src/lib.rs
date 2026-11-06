//! The Interstice benchmark suite (see `benchmarks/README.md`).
//!
//! Built after SpacetimeDB's, so the numbers can be put side by side:
//! - `benches/generic.rs`: the `crates/bench` matrix (raw datastore, a WASM
//!   module called in process, SQLite) with Criterion;
//! - `interstice-bench transfer`: the keynote transfer load, counting
//!   committed calls and their latency at the client.

pub mod data;
pub mod node;
pub mod stats;
pub mod targets;
pub mod transfer;
pub mod zipf;
