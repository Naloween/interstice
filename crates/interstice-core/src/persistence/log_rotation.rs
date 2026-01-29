//! Log rotation for managing large transaction logs.
//!
//! Provides automatic rotation of transaction logs when they exceed
//! a configurable size, keeping older logs for archival.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::IntersticeError;

/// Configuration for log rotation behavior
#[derive(Debug, Clone)]
pub struct RotationConfig {
    /// Maximum size of a single log file in bytes (default: 100MB)
    pub max_log_size: u64,
    /// Maximum number of rotated logs to keep (default: 10)
    pub max_rotated_logs: usize,
}

impl Default for RotationConfig {
    fn default() -> Self {
        Self {
            max_log_size: 100 * 1024 * 1024, // 100MB
            max_rotated_logs: 10,
        }
    }
}

/// Manages log rotation for transaction logs
pub struct LogRotator {
    config: RotationConfig,
}

impl LogRotator {
    /// Create a new log rotator with custom configuration
    pub fn new(config: RotationConfig) -> Self {
        Self { config }
    }

    /// Check if a log file needs rotation based on size
    pub fn should_rotate(&self, log_path: &Path) -> Result<bool, IntersticeError> {
        match fs::metadata(log_path) {
            Ok(metadata) => Ok(metadata.len() > self.config.max_log_size),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(IntersticeError::Internal(format!(
                "Shouold rotate log error: {}",
                e
            ))),
        }
    }

    /// Rotate a log file by renaming it and starting a new one
    ///
    /// Example:
    /// - `txn.log` becomes `txn.log.0`
    /// - `txn.log.0` becomes `txn.log.1`
    /// - etc., up to `max_rotated_logs`
    pub fn rotate(&self, log_path: &Path) -> Result<(), IntersticeError> {
        if !log_path.exists() {
            return Ok(()); // Nothing to rotate
        }

        let dir = log_path
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Invalid log path"))
            .map_err(|err| {
                IntersticeError::Internal(format!(
                    "Couldn't get transaction log file path: {}",
                    err
                ))
            })?;

        let base_name_str = log_path
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "No filename"))
            .map_err(|err| {
                IntersticeError::Internal(format!(
                    "Couldn't retreive base name transaction log file: {}",
                    err
                ))
            })?
            .to_string_lossy();

        // Find the max existing numbered log
        let mut max_num: i32 = -1;
        for entry in fs::read_dir(dir).map_err(|err| {
            IntersticeError::Internal(format!(
                "Couldn't retreive the number of transaction log files: {}",
                err
            ))
        })? {
            let entry = entry.map_err(|err| {
                IntersticeError::Internal(format!(
                    "Couldn't get transaction log file rotation: {}",
                    err
                ))
            })?;
            let path = entry.path();
            if let Some(name) = path.file_name() {
                let name_str = name.to_string_lossy();
                if let Some(num_str) = name_str.strip_prefix(&format!("{}.", base_name_str)) {
                    if let Ok(num) = num_str.parse::<i32>() {
                        max_num = max_num.max(num);
                    }
                }
            }
        }

        // Rotate backwards from the highest number
        // When max_num is -1 (no numbered logs exist), start with i=0 (the base log)
        for i in (0..=max_num.max(-1)).rev() {
            let new_num = i + 1;

            // Skip if exceeds retention limit
            if new_num as usize >= self.config.max_rotated_logs {
                let path_to_remove = if i == -1 {
                    log_path.to_path_buf()
                } else {
                    dir.join(format!("{}.{}", base_name_str, i))
                };
                if path_to_remove.exists() {
                    fs::remove_file(&path_to_remove).ok();
                }
                continue;
            }

            let old_path = if i == -1 {
                log_path.to_path_buf()
            } else {
                dir.join(format!("{}.{}", base_name_str, i))
            };

            let new_path = dir.join(format!("{}.{}", base_name_str, new_num));

            if old_path.exists() {
                fs::rename(&old_path, &new_path).map_err(|err| {
                    IntersticeError::Internal(format!(
                        "Couldn't sync transaction log file to disk: {}",
                        err
                    ))
                })?;
            }
        }

        Ok(())
    }

    /// List all rotated logs for a given path
    pub fn list_rotated_logs(&self, log_path: &Path) -> io::Result<Vec<PathBuf>> {
        let dir = log_path
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Invalid log path"))?;

        let base_name = log_path
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "No filename"))?
            .to_string_lossy();

        let mut logs = Vec::new();

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if let Some(name) = path.file_name() {
                let name_str = name.to_string_lossy();
                if name_str.starts_with(&format!("{}.", base_name)) {
                    logs.push(path);
                }
            }
        }

        logs.sort();
        Ok(logs)
    }
}
