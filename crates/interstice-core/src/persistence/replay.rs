//! Replay engine for reconstructing state from transaction logs.
//!
//! The ReplayEngine reads a transaction log and applies all mutations directly
//! to table state, without executing any reducer logic. This reconstructs the
//! exact state before a shutdown or crash.

use super::transaction_log::TransactionLog;
use super::types::Transaction;
use std::io;

/// Replays transactions from a log to reconstruct previous state.
///
/// Does not execute reducers or trigger subscriptions - only applies mutations.
pub struct ReplayEngine {
    log: TransactionLog,
}

impl ReplayEngine {
    /// Create a replay engine from a transaction log file
    pub fn new(log: TransactionLog) -> Self {
        Self { log }
    }

    /// Replay all transactions from the log, returning the mutations in order
    ///
    /// Each returned transaction represents a single mutation that was applied.
    /// This can be used to reconstruct table state without running reducer logic.
    pub fn replay_all_transactions(&self) -> io::Result<Vec<Transaction>> {
        self.log.read_all()
    }

    /// Get count of transactions in the log
    pub fn transaction_count(&self) -> usize {
        self.log.transaction_count()
    }

    /// Verify the log can be read without corruption
    ///
    /// Returns Ok(count) if all transactions are valid,
    /// or an error if any transaction is corrupted.
    pub fn verify(&self) -> io::Result<usize> {
        let txs = self.replay_all_transactions()?;
        Ok(txs.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::TransactionType;
    use interstice_abi::{IntersticeValue, Row};

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
    fn test_replay_engine_creation() {
        let tmpdir = tempfile::tempdir().unwrap();
        let log_path = tmpdir.path().join("replay.log");

        let mut log = TransactionLog::new(&log_path).unwrap();
        log.append(&make_tx("hello", "greetings", 1, "Hello, World!"))
            .unwrap();

        let engine = ReplayEngine::new(log);
        assert_eq!(engine.transaction_count(), 1);
    }

    #[test]
    fn test_replay_engine_verify() {
        let tmpdir = tempfile::tempdir().unwrap();
        let log_path = tmpdir.path().join("verify.log");

        let mut log = TransactionLog::new(&log_path).unwrap();
        log.append(&make_tx("hello", "greetings", 1, "Greeting 1"))
            .unwrap();
        log.append(&make_tx("hello", "greetings", 2, "Greeting 2"))
            .unwrap();

        let engine = ReplayEngine::new(log);
        assert_eq!(engine.verify().unwrap(), 2);
    }

    #[test]
    fn test_replay_engine_read_all() {
        let tmpdir = tempfile::tempdir().unwrap();
        let log_path = tmpdir.path().join("read_all.log");

        let mut log = TransactionLog::new(&log_path).unwrap();

        let tx1 = make_tx("hello", "greetings", 1, "Greeting 1");
        let tx2 = make_tx("hello", "greetings", 2, "Greeting 2");

        log.append(&tx1).unwrap();
        log.append(&tx2).unwrap();

        let engine = ReplayEngine::new(log);
        let txs = engine.replay_all_transactions().unwrap();

        assert_eq!(txs.len(), 2);
        assert_eq!(txs[0].row, tx1.row);
        assert_eq!(txs[1].row, tx2.row);
    }

    #[test]
    fn test_replay_engine_with_updates_and_deletes() {
        let tmpdir = tempfile::tempdir().unwrap();
        let log_path = tmpdir.path().join("mixed.log");

        let mut log = TransactionLog::new(&log_path).unwrap();

        // Insert
        let insert_tx = make_tx("hello", "greetings", 1, "Original");
        log.append(&insert_tx).unwrap();

        // Update
        let update_tx = Transaction {
            transaction_type: TransactionType::Update,
            module_name: "hello".to_string(),
            table_name: "greetings".to_string(),
            row: make_row(1, "Updated"),
            old_row: Some(make_row(1, "Original")),
            timestamp: 2,
        };
        log.append(&update_tx).unwrap();

        // Delete
        let delete_tx = Transaction {
            transaction_type: TransactionType::Delete,
            module_name: "hello".to_string(),
            table_name: "greetings".to_string(),
            row: make_row(1, "Updated"),
            old_row: None,
            timestamp: 3,
        };
        log.append(&delete_tx).unwrap();

        let engine = ReplayEngine::new(log);
        let txs = engine.replay_all_transactions().unwrap();

        assert_eq!(txs.len(), 3);
        assert_eq!(txs[0].transaction_type, TransactionType::Insert);
        assert_eq!(txs[1].transaction_type, TransactionType::Update);
        assert_eq!(txs[2].transaction_type, TransactionType::Delete);
    }
}
