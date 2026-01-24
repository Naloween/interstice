//! Persistence layer for Interstice
//!
//! This module provides durable storage of transactions via an append-only log.
//! Key responsibilities:
//! - Write mutations to disk before acknowledging them
//! - Validate and recover from corrupted logs
//! - Enable replay of the log to reconstruct state

mod config;
mod log_rotation;
mod log_validation;
mod migration;
mod replay;
mod schema_versioning;
mod transaction_log;
mod types;
pub mod determinism;

pub use config::PersistenceConfig;
pub use log_rotation::{LogRotator, RotationConfig};
pub use log_validation::{LogInfo, LogValidator, ValidationResult};
pub use migration::{MigrationRegistry, TableMigration, MigrationRecord};
pub use replay::ReplayEngine;
pub use schema_versioning::SchemaVersionRegistry;
pub use transaction_log::TransactionLog;
pub use types::{Transaction, TransactionType};
pub use determinism::{DeterminismCheckResult, check_determinism};

#[cfg(test)]
mod tests;
