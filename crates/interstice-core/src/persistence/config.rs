//! Persistence configuration for the runtime.
//!
//! Controls how transactions are logged and durability behavior.

use std::path::PathBuf;

/// Configuration for transaction log persistence.
#[derive(Debug, Clone)]
pub struct PersistenceConfig {
    /// Enable transaction logging (default: true)
    pub enabled: bool,
    /// Path to log directory (default: "./interstice_logs")
    pub log_dir: PathBuf,
    /// Sync to disk on every append (default: true, safest but slower)
    pub sync_on_append: bool,
}

impl PersistenceConfig {
    /// Create with default settings (persistence enabled, safe mode)
    pub fn default_safe() -> Self {
        Self {
            enabled: true,
            log_dir: PathBuf::from("./interstice_logs"),
            sync_on_append: true,
        }
    }

    /// Create with default settings but faster (buffered writes)
    pub fn default_fast() -> Self {
        Self {
            enabled: true,
            log_dir: PathBuf::from("./interstice_logs"),
            sync_on_append: false,
        }
    }

    /// Create with persistence disabled (in-memory only)
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            log_dir: PathBuf::from("./interstice_logs"),
            sync_on_append: true,
        }
    }

    /// Set custom log directory
    pub fn with_log_dir(mut self, dir: PathBuf) -> Self {
        self.log_dir = dir;
        self
    }

    /// Get the path to the main transaction log file
    pub fn log_file_path(&self) -> PathBuf {
        self.log_dir.join("transactions.log")
    }
}

impl Default for PersistenceConfig {
    fn default() -> Self {
        Self::default_safe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let cfg = PersistenceConfig::default();
        assert!(cfg.enabled);
        assert!(cfg.sync_on_append);
    }

    #[test]
    fn test_custom_config() {
        let cfg = PersistenceConfig::disabled().with_log_dir(PathBuf::from("/tmp/logs"));
        assert!(!cfg.enabled);
        assert_eq!(cfg.log_dir, PathBuf::from("/tmp/logs"));
    }

    #[test]
    fn test_log_file_path() {
        let cfg = PersistenceConfig::default().with_log_dir(PathBuf::from("./data"));
        assert_eq!(cfg.log_file_path(), PathBuf::from("./data/transactions.log"));
    }
}
