//! Append-only transaction log implementation
//!
//! The log stores transactions in a binary format with checksums for integrity.
//! Format per transaction:
//! - version: u8 (1 byte)
//! - type: u8 (1 byte)
//! - module_name_len: u32 (4 bytes)
//! - module_name: bytes
//! - table_name_len: u32 (4 bytes)
//! - table_name: bytes
//! - row_data: bincode-encoded Row
//! - old_row_data: bincode-encoded Option<Row>
//! - timestamp: u64 (8 bytes)
//! - checksum: u32 (4 bytes, CRC32)

use super::types::{Transaction, TransactionType, LOG_FORMAT_VERSION};
use super::log_rotation::{LogRotator, RotationConfig};
use interstice_abi::Row;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Append-only transaction log for durable storage
pub struct TransactionLog {
    file: Arc<Mutex<File>>,
    path: PathBuf,
    /// Number of transactions recorded (in-memory cache)
    tx_count: usize,
    /// Log rotator for managing file size
    rotator: LogRotator,
}

impl TransactionLog {
    /// Create or open a transaction log at the given path
    pub fn new<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        Self::with_rotation(path, RotationConfig::default())
    }

    /// Create or open a transaction log with custom rotation config
    pub fn with_rotation<P: AsRef<Path>>(path: P, rotation_config: RotationConfig) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();

        // Open or create the file
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)?;

        // Count existing transactions
        let tx_count = Self::count_transactions(&file)?;

        let rotator = LogRotator::new(rotation_config);

        Ok(Self {
            file: Arc::new(Mutex::new(file)),
            path,
            tx_count,
            rotator,
        })
    }

    /// Append a transaction to the log
    ///
    /// Automatically rotates the log if it exceeds the configured size.
    ///
    /// # Arguments
    /// * `tx` - The transaction to record
    ///
    /// # Returns
    /// Ok(()) if successfully written and synced to disk
    pub fn append(&mut self, tx: &Transaction) -> io::Result<()> {
        let encoded = self.encode_transaction(tx)?;

        let mut file = self.file.lock().unwrap();

        // Seek to end of file
        file.seek(SeekFrom::End(0))?;

        // Write encoded transaction
        file.write_all(&encoded)?;

        // Ensure data is synced to disk
        file.sync_all()?;

        drop(file); // Release lock before rotation

        self.tx_count += 1;

        // Check if rotation is needed
        if self.rotator.should_rotate(&self.path)? {
            self.rotator.rotate(&self.path)?;
            // Reopen the file after rotation
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(&self.path)?;
            self.file = Arc::new(Mutex::new(file));
        }

        Ok(())
    }

    /// Read all transactions from the log
    ///
    /// # Returns
    /// A vector of all transactions in order, or an error if any are corrupted
    pub fn read_all(&self) -> io::Result<Vec<Transaction>> {
        let mut file = self.file.lock().unwrap();
        file.seek(SeekFrom::Start(0))?;

        let mut transactions = Vec::new();
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;

        let mut offset = 0;
        while offset < buffer.len() {
            match self.decode_transaction(&buffer[offset..]) {
                Ok((tx, bytes_read)) => {
                    transactions.push(tx);
                    offset += bytes_read;
                }
                Err(e) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("Transaction corruption at offset {}: {}", offset, e),
                    ));
                }
            }
        }

        Ok(transactions)
    }

    /// Get the number of transactions in the log
    pub fn transaction_count(&self) -> usize {
        self.tx_count
    }

    /// Get the path to the log file
    pub fn path(&self) -> &Path {
        &self.path
    }

    // Internal helper: encode a transaction to bytes
    fn encode_transaction(&self, tx: &Transaction) -> io::Result<Vec<u8>> {
        use std::io::Cursor;

        let mut buf = Cursor::new(Vec::new());

        // Write format version
        buf.write_all(&[LOG_FORMAT_VERSION])?;

        // Write transaction type
        buf.write_all(&[tx.transaction_type.to_byte()])?;

        // Write module name (length-prefixed)
        let module_bytes = tx.module_name.as_bytes();
        buf.write_all(&(module_bytes.len() as u32).to_le_bytes())?;
        buf.write_all(module_bytes)?;

        // Write table name (length-prefixed)
        let table_bytes = tx.table_name.as_bytes();
        buf.write_all(&(table_bytes.len() as u32).to_le_bytes())?;
        buf.write_all(table_bytes)?;

        // Encode row using bincode
        let row_encoded = bincode::serialize(&tx.row)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        buf.write_all(&(row_encoded.len() as u32).to_le_bytes())?;
        buf.write_all(&row_encoded)?;

        // Encode optional old_row
        let old_row_encoded = bincode::serialize(&tx.old_row)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        buf.write_all(&(old_row_encoded.len() as u32).to_le_bytes())?;
        buf.write_all(&old_row_encoded)?;

        // Write timestamp
        buf.write_all(&tx.timestamp.to_le_bytes())?;

        // Calculate and write checksum
        let data = buf.get_ref();
        let checksum = crc32fast::hash(&data);
        buf.write_all(&checksum.to_le_bytes())?;

        Ok(buf.into_inner())
    }

    // Internal helper: decode a transaction from bytes
    fn decode_transaction(&self, bytes: &[u8]) -> io::Result<(Transaction, usize)> {
        use std::io::Cursor;

        let mut cursor = Cursor::new(bytes);

        // Read and verify version
        let mut version_buf = [0u8; 1];
        cursor.read_exact(&mut version_buf)?;
        if version_buf[0] != LOG_FORMAT_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Incompatible log version",
            ));
        }

        // Read transaction type
        let mut type_buf = [0u8; 1];
        cursor.read_exact(&mut type_buf)?;
        let transaction_type = TransactionType::from_byte(type_buf[0]).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "Invalid transaction type")
        })?;

        // Read module name
        let mut len_buf = [0u8; 4];
        cursor.read_exact(&mut len_buf)?;
        let module_name_len = u32::from_le_bytes(len_buf) as usize;
        let mut module_name_buf = vec![0u8; module_name_len];
        cursor.read_exact(&mut module_name_buf)?;
        let module_name = String::from_utf8(module_name_buf)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;

        // Read table name
        cursor.read_exact(&mut len_buf)?;
        let table_name_len = u32::from_le_bytes(len_buf) as usize;
        let mut table_name_buf = vec![0u8; table_name_len];
        cursor.read_exact(&mut table_name_buf)?;
        let table_name = String::from_utf8(table_name_buf)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;

        // Read row
        cursor.read_exact(&mut len_buf)?;
        let row_len = u32::from_le_bytes(len_buf) as usize;
        let mut row_buf = vec![0u8; row_len];
        cursor.read_exact(&mut row_buf)?;
        let row: Row = bincode::deserialize(&row_buf)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;

        // Read optional old_row
        cursor.read_exact(&mut len_buf)?;
        let old_row_len = u32::from_le_bytes(len_buf) as usize;
        let mut old_row_buf = vec![0u8; old_row_len];
        cursor.read_exact(&mut old_row_buf)?;
        let old_row: Option<Row> = bincode::deserialize(&old_row_buf)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;

        // Read timestamp
        let mut timestamp_buf = [0u8; 8];
        cursor.read_exact(&mut timestamp_buf)?;
        let timestamp = u64::from_le_bytes(timestamp_buf);

        // Read and verify checksum
        let mut checksum_buf = [0u8; 4];
        cursor.read_exact(&mut checksum_buf)?;
        let stored_checksum = u32::from_le_bytes(checksum_buf);

        let pos = cursor.position() as usize;
        let data_to_check = &bytes[..pos - 4]; // Exclude checksum itself
        let calculated_checksum = crc32fast::hash(data_to_check);

        if stored_checksum != calculated_checksum {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Checksum mismatch: expected {}, got {}",
                    calculated_checksum, stored_checksum
                ),
            ));
        }

        let bytes_read = pos;
        Ok((
            Transaction {
                transaction_type,
                module_name,
                table_name,
                row,
                old_row,
                timestamp,
            },
            bytes_read,
        ))
    }

    // Count existing transactions in file
    fn count_transactions(file: &File) -> io::Result<usize> {
        let mut reader = file;
        let mut count = 0;
        let mut buffer = Vec::new();
        reader.read_to_end(&mut buffer)?;

        let mut offset = 0;
        while offset < buffer.len() {
            // Try to read version
            if offset + 1 > buffer.len() {
                break;
            }

            let version = buffer[offset];
            if version != LOG_FORMAT_VERSION {
                break;
            }

            // Try to decode this transaction to count bytes
            let temp_log = TransactionLog {
                file: Arc::new(Mutex::new(file.try_clone()?)),
                path: PathBuf::new(),
                tx_count: 0,
                rotator: LogRotator::new(RotationConfig::default()),
            };

            match temp_log.decode_transaction(&buffer[offset..]) {
                Ok((_, bytes_read)) => {
                    offset += bytes_read;
                    count += 1;
                }
                Err(_) => break,
            }
        }

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use interstice_abi::Row;

    fn test_transaction() -> Transaction {
        Transaction {
            transaction_type: TransactionType::Insert,
            module_name: "test_module".to_string(),
            table_name: "test_table".to_string(),
            row: Row {
                primary_key: interstice_abi::IntersticeValue::U64(1),
                entries: vec![interstice_abi::IntersticeValue::String("test".to_string())],
            },
            old_row: None,
            timestamp: 12345,
        }
    }

    #[test]
    fn test_create_log() {
        let tmpdir = tempfile::tempdir().unwrap();
        let log_path = tmpdir.path().join("test.log");

        let log = TransactionLog::new(&log_path).unwrap();
        assert_eq!(log.transaction_count(), 0);
        assert!(log.path().exists());
    }

    #[test]
    fn test_append_and_read() {
        let tmpdir = tempfile::tempdir().unwrap();
        let log_path = tmpdir.path().join("test.log");

        let mut log = TransactionLog::new(&log_path).unwrap();
        let tx = test_transaction();

        log.append(&tx).unwrap();
        assert_eq!(log.transaction_count(), 1);

        let transactions = log.read_all().unwrap();
        assert_eq!(transactions.len(), 1);
        assert_eq!(transactions[0], tx);
    }

    #[test]
    fn test_multiple_transactions() {
        let tmpdir = tempfile::tempdir().unwrap();
        let log_path = tmpdir.path().join("test.log");

        let mut log = TransactionLog::new(&log_path).unwrap();

        let mut txs = vec![];
        for i in 0..5 {
            let mut tx = test_transaction();
            tx.timestamp = i as u64;
            txs.push(tx.clone());
            log.append(&tx).unwrap();
        }

        assert_eq!(log.transaction_count(), 5);

        let read_txs = log.read_all().unwrap();
        assert_eq!(read_txs, txs);
    }

    #[test]
    fn test_persistence_across_instances() {
        let tmpdir = tempfile::tempdir().unwrap();
        let log_path = tmpdir.path().join("test.log");

        let tx = test_transaction();

        // Write with one instance
        {
            let mut log = TransactionLog::new(&log_path).unwrap();
            log.append(&tx).unwrap();
        }

        // Read with another instance
        {
            let log = TransactionLog::new(&log_path).unwrap();
            assert_eq!(log.transaction_count(), 1);
            let txs = log.read_all().unwrap();
            assert_eq!(txs.len(), 1);
            assert_eq!(txs[0], tx);
        }
    }

    #[test]
    fn test_update_transaction() {
        let tmpdir = tempfile::tempdir().unwrap();
        let log_path = tmpdir.path().join("test.log");

        let mut log = TransactionLog::new(&log_path).unwrap();

        let old_row = Row {
            primary_key: interstice_abi::IntersticeValue::U64(1),
            entries: vec![interstice_abi::IntersticeValue::String("old".to_string())],
        };

        let new_row = Row {
            primary_key: interstice_abi::IntersticeValue::U64(1),
            entries: vec![interstice_abi::IntersticeValue::String("new".to_string())],
        };

        let tx = Transaction {
            transaction_type: TransactionType::Update,
            module_name: "test".to_string(),
            table_name: "test".to_string(),
            row: new_row.clone(),
            old_row: Some(old_row.clone()),
            timestamp: 100,
        };

        log.append(&tx).unwrap();
        let txs = log.read_all().unwrap();
        assert_eq!(txs[0].old_row, Some(old_row));
    }
}
