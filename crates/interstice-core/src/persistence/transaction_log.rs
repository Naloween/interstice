use super::log_rotation::{LogRotator, RotationConfig};
use crate::error::IntersticeError;
use crate::runtime::transaction::Transaction;
use interstice_abi::decode;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

enum LogStorage {
    File {
        file: Arc<Mutex<File>>,
        path: PathBuf,
        tx_count: usize,
        rotator: LogRotator,
    },
    Memory {
        transactions: Vec<Transaction>,
    },
}

/// Append-only transaction log for durable storage
pub struct TransactionLog {
    storage: LogStorage,
}

impl TransactionLog {
    /// Create or open a transaction log at the given path
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, IntersticeError> {
        Self::with_rotation(path, RotationConfig::default())
    }

    /// Create or open a transaction log with custom rotation config
    pub fn with_rotation<P: AsRef<Path>>(
        path: P,
        rotation_config: RotationConfig,
    ) -> Result<Self, IntersticeError> {
        let path = path.as_ref().to_path_buf();

        // Open or create the file
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)
            .map_err(|err| {
                IntersticeError::Internal(format!("Couldn't open transaction log file: {}", err))
            })?;

        let rotator = LogRotator::new(rotation_config);
        let mut transaction_log = Self {
            storage: LogStorage::File {
                file: Arc::new(Mutex::new(file)),
                path,
                tx_count: 0,
                rotator,
            },
        };

        // Count existing transactions
        let count = transaction_log.read_all()?.len();
        if let LogStorage::File { tx_count, .. } = &mut transaction_log.storage {
            *tx_count = count;
        }

        Ok(transaction_log)
    }

    pub fn new_in_memory() -> Self {
        Self {
            storage: LogStorage::Memory {
                transactions: Vec::new(),
            },
        }
    }

    pub fn append(&mut self, tx: &Transaction) -> Result<(), IntersticeError> {
        match &mut self.storage {
            LogStorage::File {
                file,
                path,
                tx_count,
                rotator,
            } => {
                let mut file_guard = file.lock().unwrap();

                // Seek to end of file
                file_guard.seek(SeekFrom::End(0)).map_err(|err| {
                    IntersticeError::Internal(format!(
                        "Couldn't seek log transaction end file: {}",
                        err
                    ))
                })?;

                let encoded = tx.encode()?;
                // Write encoded transaction and its length in bytes
                let length = (encoded.len() as u32).to_le_bytes();
                file_guard.write_all(&length).map_err(|err| {
                    IntersticeError::Internal(format!(
                        "Couldn't write transaction to log file: {}",
                        err
                    ))
                })?;
                file_guard.write_all(&encoded).map_err(|err| {
                    IntersticeError::Internal(format!(
                        "Couldn't write transaction to log file: {}",
                        err
                    ))
                })?;

                // Ensure data is synced to disk
                file_guard.sync_all().map_err(|err| {
                    IntersticeError::Internal(format!(
                        "Couldn't sync transaction log file to disk: {}",
                        err
                    ))
                })?;

                drop(file_guard);

                *tx_count += 1;

                // Check if rotation is needed
                if rotator.should_rotate(path)? {
                    rotator.rotate(path)?;
                    // Reopen the file after rotation
                    let reopened = OpenOptions::new()
                        .read(true)
                        .write(true)
                        .create(true)
                        .open(path)
                        .map_err(|err| {
                            IntersticeError::Internal(format!(
                                "Couldn't open new log transaction file: {}",
                                err
                            ))
                        })?;
                    *file = Arc::new(Mutex::new(reopened));
                }

                Ok(())
            }
            LogStorage::Memory { transactions } => {
                transactions.push(tx.clone());
                Ok(())
            }
        }
    }

    pub fn read_all(&self) -> Result<Vec<Transaction>, IntersticeError> {
        match &self.storage {
            LogStorage::File { file, .. } => {
                let mut file = file.lock().unwrap();
                file.seek(SeekFrom::Start(0)).map_err(|err| {
                    IntersticeError::Internal(format!(
                        "Error when opening transaction logs file: {}",
                        err
                    ))
                })?;

                let mut transactions = Vec::new();
                loop {
                    let mut length_buf = [0; 4];
                    if file.read_exact(&mut length_buf).is_err() {
                        break;
                    }

                    let length = u32::from_le_bytes(length_buf) as usize;
                    let mut encoded = vec![0; length];

                    file.read_exact(&mut encoded).map_err(|err| {
                        IntersticeError::Internal(format!(
                            "Error when decoding transaction logs: {}",
                            err
                        ))
                    })?;

                    let transaction = decode(&encoded).map_err(|err| {
                        IntersticeError::Internal(format!(
                            "Error when decoding transaction logs: {}",
                            err
                        ))
                    })?;
                    transactions.push(transaction);
                }

                Ok(transactions)
            }
            LogStorage::Memory { transactions } => Ok(transactions.clone()),
        }
    }

    /// Get the number of transactions in the log
    pub fn transaction_count(&self) -> usize {
        match &self.storage {
            LogStorage::File { tx_count, .. } => *tx_count,
            LogStorage::Memory { transactions } => transactions.len(),
        }
    }

    /// Get the path to the log file
    pub fn path(&self) -> Option<&Path> {
        match &self.storage {
            LogStorage::File { path, .. } => Some(path.as_path()),
            LogStorage::Memory { .. } => None,
        }
    }

    pub fn delete_all_logs(&mut self) -> Result<(), IntersticeError> {
        match &mut self.storage {
            LogStorage::File { path, .. } => {
                let mut file = OpenOptions::new()
                    .write(true)
                    .truncate(true)
                    .open(path)
                    .map_err(|err| {
                        IntersticeError::Internal(format!(
                            "Error when opening transaction logs: {}",
                            err
                        ))
                    })?;

                file.write_all(&[]).map_err(|err| {
                    IntersticeError::Internal(format!(
                        "Error when deleting transaction logs: {}",
                        err
                    ))
                })?;

                Ok(())
            }
            LogStorage::Memory { transactions } => {
                transactions.clear();
                Ok(())
            }
        }
    }
}
