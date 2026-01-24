//! Integration tests for persistence layer
//!
//! These tests verify that the transaction log works correctly with
//! real table operations and module scenarios.

#[cfg(test)]
mod integration_tests {
    use crate::persistence::{Transaction, TransactionLog, TransactionType};
    use interstice_abi::{IntersticeValue, Row};

    /// Helper: create a test row with given ID and greeting text
    fn make_greeting_row(id: u64, text: &str) -> Row {
        Row {
            primary_key: IntersticeValue::U64(id),
            entries: vec![IntersticeValue::String(text.to_string())],
        }
    }

    /// Helper: create a test transaction for greeting insert
    fn make_insert_tx(id: u64, text: &str) -> Transaction {
        Transaction {
            transaction_type: TransactionType::Insert,
            module_name: "hello".to_string(),
            table_name: "greetings".to_string(),
            row: make_greeting_row(id, text),
            old_row: None,
            timestamp: id,
        }
    }

    #[test]
    fn test_log_hello_module_greetings() {
        let tmpdir = tempfile::tempdir().unwrap();
        let log_path = tmpdir.path().join("hello_greetings.log");

        let mut log = TransactionLog::new(&log_path).unwrap();

        // Simulate hello module inserting greetings
        let tx1 = make_insert_tx(1, "Hello, World!");
        let tx2 = make_insert_tx(2, "Hello, Rust!");
        let tx3 = make_insert_tx(3, "Hello, Interstice!");

        log.append(&tx1).unwrap();
        log.append(&tx2).unwrap();
        log.append(&tx3).unwrap();

        // Verify all greetings are recorded
        let txs = log.read_all().unwrap();
        assert_eq!(txs.len(), 3);
        assert_eq!(txs[0].row, tx1.row);
        assert_eq!(txs[1].row, tx2.row);
        assert_eq!(txs[2].row, tx3.row);
    }

    #[test]
    fn test_log_caller_to_hello_flow() {
        let tmpdir = tempfile::tempdir().unwrap();
        let log_path = tmpdir.path().join("caller_flow.log");

        let mut log = TransactionLog::new(&log_path).unwrap();

        // Simulate: caller module calls hello module, which inserts greeting
        let greeting_tx = Transaction {
            transaction_type: TransactionType::Insert,
            module_name: "hello".to_string(),
            table_name: "greetings".to_string(),
            row: make_greeting_row(1, "Hello, called from caller!"),
            old_row: None,
            timestamp: 100,
        };

        log.append(&greeting_tx).unwrap();

        // Verify the transaction is persisted
        let txs = log.read_all().unwrap();
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0].module_name, "hello");
        assert_eq!(txs[0].table_name, "greetings");
    }

    #[test]
    fn test_log_update_greeting() {
        let tmpdir = tempfile::tempdir().unwrap();
        let log_path = tmpdir.path().join("update_greeting.log");

        let mut log = TransactionLog::new(&log_path).unwrap();

        let old_row = make_greeting_row(1, "Hello, World!");
        let new_row = make_greeting_row(1, "Hello, Updated!");

        let update_tx = Transaction {
            transaction_type: TransactionType::Update,
            module_name: "hello".to_string(),
            table_name: "greetings".to_string(),
            row: new_row,
            old_row: Some(old_row.clone()),
            timestamp: 200,
        };

        log.append(&update_tx).unwrap();

        // Verify old_row is preserved
        let txs = log.read_all().unwrap();
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0].old_row, Some(old_row));
    }

    #[test]
    fn test_log_delete_greeting() {
        let tmpdir = tempfile::tempdir().unwrap();
        let log_path = tmpdir.path().join("delete_greeting.log");

        let mut log = TransactionLog::new(&log_path).unwrap();

        let delete_tx = Transaction {
            transaction_type: TransactionType::Delete,
            module_name: "hello".to_string(),
            table_name: "greetings".to_string(),
            row: make_greeting_row(1, "Hello, Deleted!"),
            old_row: None,
            timestamp: 300,
        };

        log.append(&delete_tx).unwrap();

        // Verify deletion is recorded
        let txs = log.read_all().unwrap();
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0].transaction_type, TransactionType::Delete);
    }

    #[test]
    fn test_log_multiple_modules_mixed_operations() {
        let tmpdir = tempfile::tempdir().unwrap();
        let log_path = tmpdir.path().join("multi_module.log");

        let mut log = TransactionLog::new(&log_path).unwrap();

        // Hello module inserts
        log.append(&make_insert_tx(1, "Greeting 1")).unwrap();
        log.append(&make_insert_tx(2, "Greeting 2")).unwrap();

        // Simulate other module operations
        let other_tx = Transaction {
            transaction_type: TransactionType::Insert,
            module_name: "graphics".to_string(),
            table_name: "some_table".to_string(),
            row: Row {
                primary_key: IntersticeValue::U64(1),
                entries: vec![IntersticeValue::String("graphics data".to_string())],
            },
            old_row: None,
            timestamp: 10,
        };

        log.append(&other_tx).unwrap();

        // Verify all are recorded in order
        let txs = log.read_all().unwrap();
        assert_eq!(txs.len(), 3);
        assert_eq!(txs[0].module_name, "hello");
        assert_eq!(txs[1].module_name, "hello");
        assert_eq!(txs[2].module_name, "graphics");
    }

    #[test]
    fn test_log_persistence_across_restarts() {
        let tmpdir = tempfile::tempdir().unwrap();
        let log_path = tmpdir.path().join("persistence.log");

        // First "session": write greetings
        {
            let mut log = TransactionLog::new(&log_path).unwrap();
            log.append(&make_insert_tx(1, "Session 1 - Greeting 1"))
                .unwrap();
            log.append(&make_insert_tx(2, "Session 1 - Greeting 2"))
                .unwrap();
        }

        // Second "session": read back and append more
        {
            let mut log = TransactionLog::new(&log_path).unwrap();

            // Verify persistence
            let initial_txs = log.read_all().unwrap();
            assert_eq!(initial_txs.len(), 2);

            // Append new greeting
            log.append(&make_insert_tx(3, "Session 2 - Greeting 3"))
                .unwrap();
        }

        // Third "session": verify all data is there
        {
            let log = TransactionLog::new(&log_path).unwrap();
            let all_txs = log.read_all().unwrap();
            assert_eq!(all_txs.len(), 3);
        }
    }
}

