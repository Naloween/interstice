//! Log validation and inspection CLI utilities.
//!
//! Provides tools for verifying transaction log integrity and analyzing logs.

use crate::persistence::{ReplayEngine, TransactionLog};
use std::path::Path;

/// Result of log validation
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// Total transactions in log
    pub total_transactions: usize,
    /// Number of valid transactions
    pub valid_transactions: usize,
    /// Any errors found
    pub errors: Vec<String>,
}

impl ValidationResult {
    /// Check if validation was successful
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty() && self.total_transactions == self.valid_transactions
    }
}

/// Validates a transaction log
pub struct LogValidator;

impl LogValidator {
    /// Validate a transaction log file
    pub fn validate<P: AsRef<Path>>(log_path: P) -> std::io::Result<ValidationResult> {
        let log_path = log_path.as_ref();

        let log = TransactionLog::new(log_path)?;
        let engine = ReplayEngine::new(log);

        let mut errors = Vec::new();
        let total_count = engine.transaction_count();

        match engine.verify() {
            Ok(valid_count) => {
                if valid_count != total_count {
                    errors.push(format!(
                        "Incomplete log: {} of {} transactions valid",
                        valid_count, total_count
                    ));
                }
                Ok(ValidationResult {
                    total_transactions: total_count,
                    valid_transactions: valid_count,
                    errors,
                })
            }
            Err(e) => {
                errors.push(format!("Log verification failed: {}", e));
                Ok(ValidationResult {
                    total_transactions: total_count,
                    valid_transactions: 0,
                    errors,
                })
            }
        }
    }

    /// Get detailed information about a log
    pub fn inspect<P: AsRef<Path>>(log_path: P) -> std::io::Result<LogInfo> {
        let log_path = log_path.as_ref();
        let log = TransactionLog::new(log_path)?;
        let engine = ReplayEngine::new(log);

        let transactions = engine.replay_all_transactions()?;
        let file_size = std::fs::metadata(log_path)?.len();

        let mut module_names = std::collections::HashSet::new();
        let mut table_names = std::collections::HashSet::new();

        for tx in &transactions {
            module_names.insert(tx.module_name.clone());
            table_names.insert(tx.table_name.clone());
        }

        Ok(LogInfo {
            path: log_path.to_path_buf(),
            file_size_bytes: file_size,
            transaction_count: transactions.len(),
            modules: module_names.into_iter().collect(),
            tables: table_names.into_iter().collect(),
        })
    }
}

/// Information about a transaction log
#[derive(Debug, Clone)]
pub struct LogInfo {
    pub path: std::path::PathBuf,
    pub file_size_bytes: u64,
    pub transaction_count: usize,
    pub modules: Vec<String>,
    pub tables: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::{Transaction, TransactionType};
    use interstice_abi::{IntersticeValue, Row};
    use tempfile::TempDir;

    fn make_row(id: u64, text: &str) -> Row {
        Row {
            primary_key: IntersticeValue::U64(id),
            entries: vec![IntersticeValue::String(text.to_string())],
        }
    }

    fn make_tx(module: &str, table: &str, id: u64, text: &str) -> Transaction {
        Transaction {
            transaction_type: TransactionType::Insert,
            module_name: module.to_string(),
            table_name: table.to_string(),
            row: make_row(id, text),
            old_row: None,
            timestamp: id,
        }
    }

    #[test]
    fn test_validate_empty_log() {
        let tmpdir = TempDir::new().unwrap();
        let log_path = tmpdir.path().join("test.log");
        TransactionLog::new(&log_path).unwrap();

        let result = LogValidator::validate(&log_path).unwrap();
        assert!(result.is_valid());
        assert_eq!(result.total_transactions, 0);
    }

    #[test]
    fn test_validate_log_with_transactions() {
        let tmpdir = TempDir::new().unwrap();
        let log_path = tmpdir.path().join("test.log");

        let mut log = TransactionLog::new(&log_path).unwrap();
        log.append(&make_tx("hello", "greetings", 1, "Hello"))
            .unwrap();
        log.append(&make_tx("hello", "greetings", 2, "World"))
            .unwrap();

        let result = LogValidator::validate(&log_path).unwrap();
        assert!(result.is_valid());
        assert_eq!(result.total_transactions, 2);
        assert_eq!(result.valid_transactions, 2);
    }

    #[test]
    fn test_inspect_log() {
        let tmpdir = TempDir::new().unwrap();
        let log_path = tmpdir.path().join("test.log");

        let mut log = TransactionLog::new(&log_path).unwrap();
        log.append(&make_tx("module1", "table1", 1, "data1"))
            .unwrap();
        log.append(&make_tx("module1", "table2", 2, "data2"))
            .unwrap();
        log.append(&make_tx("module2", "table1", 3, "data3"))
            .unwrap();

        let info = LogValidator::inspect(&log_path).unwrap();
        assert_eq!(info.transaction_count, 3);
        assert_eq!(info.modules.len(), 2);
        assert_eq!(info.tables.len(), 2);
    }
}
