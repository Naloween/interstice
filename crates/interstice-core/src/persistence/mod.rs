//! Persistence layer for Interstice
//!
//! This module provides durable storage of transactions via an append-only log.
//! Key responsibilities:
//! - Write mutations to disk before acknowledging them
//! - Validate and recover from corrupted logs
//! - Enable replay of the log to reconstruct state

mod transaction_log;
mod types;

pub use transaction_log::TransactionLog;
pub use types::{Transaction, TransactionType};

#[cfg(test)]
mod tests;
