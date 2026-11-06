#![recursion_limit = "256"]
mod app;
mod audio;
pub mod bundle;
mod error;
mod logger;
mod network;
mod node;
pub mod persistence;
pub mod security;
mod runtime;

pub use crate::node::{Decide, Node, NodeHandle, NodeId};
pub use error::*;
pub use interstice_abi;
pub use network::packet;
pub use network::identity::{KnownPeers, LocalIdentity, SecureStream, derive_id};
pub use network::protocol::*;
pub use runtime::migration::MigrationPolicy;
pub use runtime::profile;

/// The in-memory table store, for the raw target of `interstice-bench`
/// (`benchmarks/README.md`). Not a stable API.
#[doc(hidden)]
pub mod datastore {
    pub use crate::runtime::table::{Table, TableOverlay};
}
