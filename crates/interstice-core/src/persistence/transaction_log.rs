use super::log_rotation::{LogRotator, RotationConfig};
use crate::error::IntersticeError;
use crate::runtime::transaction::Transaction;
use interstice_abi::decode;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
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
            file: Arc::new(Mutex::new(file)),
            path,
            tx_count: 0,
            rotator,
        };

        // Count existing transactions
        transaction_log.tx_count = transaction_log.read_all()?.len();

        Ok(transaction_log)
    }

    pub fn append(&mut self, tx: &Transaction) -> Result<(), IntersticeError> {
        let mut file = self.file.lock().unwrap();

        // Seek to end of file
        file.seek(SeekFrom::End(0)).map_err(|err| {
            IntersticeError::Internal(format!("Couldn't seek log transaction end file: {}", err))
        })?;

        let encoded = tx.encode()?;
        // Write encoded transaction and its length in bytes
        let length = (encoded.len() as u32).to_le_bytes(); // Convert length to bytes
        file.write_all(&length).map_err(|err| {
            IntersticeError::Internal(format!("Couldn't write transaction to log file: {}", err))
        })?;
        file.write_all(&encoded).map_err(|err| {
            IntersticeError::Internal(format!("Couldn't write transaction to log file: {}", err))
        })?;

        // Ensure data is synced to disk
        file.sync_all().map_err(|err| {
            IntersticeError::Internal(format!(
                "Couldn't sync transaction log file to disk: {}",
                err
            ))
        })?;

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
                .open(&self.path)
                .map_err(|err| {
                    IntersticeError::Internal(format!(
                        "Couldn't open new log transaction file: {}",
                        err
                    ))
                })?;
            self.file = Arc::new(Mutex::new(file));
        }

        Ok(())
    }

    pub fn read_all(&self) -> Result<Vec<Transaction>, IntersticeError> {
        let mut file = self.file.lock().unwrap();
        file.seek(SeekFrom::Start(0)).map_err(|err| {
            IntersticeError::Internal(format!("Error when opening transaction logs file: {}", err))
        })?;

        let mut transactions = Vec::new();
        loop {
            // Read the length of the next object
            let mut length_buf = [0; 4]; // Buffer for length (4 bytes for u32)
            if file.read_exact(&mut length_buf).is_err() {
                break; // Exit loop if the end of the file is reached
            }

            let length = u32::from_le_bytes(length_buf) as usize; // Convert bytes to usize
            let mut encoded = vec![0; length]; // Create a buffer for the encoded data

            // Read the encoded object
            file.read_exact(&mut encoded).map_err(|err| {
                IntersticeError::Internal(format!("Error when decoding transaction logs: {}", err))
            })?;

            // Deserialize the object
            let transaction = decode(&encoded).map_err(|err| {
                IntersticeError::Internal(format!("Error when decoding transaction logs: {}", err))
            })?;
            transactions.push(transaction);
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

    pub fn delete_all_logs(&self) -> Result<(), IntersticeError> {
        // Open the file with write and truncate options
        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true) // Truncate the file to zero length
            .open(self.path()) // Assuming `self.file_path` is the path to the log file
            .map_err(|err| {
                IntersticeError::Internal(format!("Error when opening transaction logs: {}", err))
            })?;

        // Optionally, you can also write an empty byte buffer if you want to ensure it's empty
        file.write_all(&[]).map_err(|err| {
            IntersticeError::Internal(format!("Error when deleting transaction logs: {}", err))
        })?;

        Ok(())
    }
}
