//! Persistence layer for Interstice
//!
//! This module provides durable storage of transactions via an append-only log.
//! Key responsibilities:
//! - Write mutations to disk before acknowledging them
//! - Validate and recover from corrupted logs
//! - Enable replay of the log to reconstruct state

mod config;
mod log_rotation;
mod peer_tokens;
mod table_store;

pub use config::PersistenceConfig;
pub use log_rotation::{LogRotator, RotationConfig};
pub use peer_tokens::PeerTokenStore;
pub use table_store::{LogOperation, SnapshotPlan, TableStore};
