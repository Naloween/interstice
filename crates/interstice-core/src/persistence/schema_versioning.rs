//! Schema versioning and validation for transaction logs.
//!
//! Tracks schema versions in the log and ensures compatibility during replay.

use std::collections::HashMap;

/// Schema version information for a module
#[derive(Debug, Clone)]
pub struct SchemaVersion {
    /// Module name
    pub module: String,
    /// Version number (starts at 0)
    pub version: u32,
    /// Timestamp when this version was recorded
    pub timestamp: u64,
}

/// Tracks schema versions throughout the transaction log
#[derive(Debug, Clone, Default)]
pub struct SchemaVersionRegistry {
    /// Versions per module
    versions: HashMap<String, Vec<SchemaVersion>>,
}

impl SchemaVersionRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            versions: HashMap::new(),
        }
    }

    /// Record a schema version
    pub fn record_version(&mut self, module: String, version: u32, timestamp: u64) {
        self.versions
            .entry(module.clone())
            .or_insert_with(Vec::new)
            .push(SchemaVersion {
                module,
                version,
                timestamp,
            });
    }

    /// Get all versions for a module
    pub fn get_versions(&self, module: &str) -> Option<&[SchemaVersion]> {
        self.versions.get(module).map(|v| v.as_slice())
    }

    /// Check if a version transition is valid
    pub fn is_compatible(&self, module: &str, from_version: u32, to_version: u32) -> bool {
        // For now, only allow incrementing by 1
        // In a real system, this would check migration rules
        to_version == from_version || to_version == from_version + 1
    }

    /// Get the latest version for a module
    pub fn latest_version(&self, module: &str) -> Option<u32> {
        self.versions
            .get(module)
            .and_then(|versions| versions.last())
            .map(|v| v.version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = SchemaVersionRegistry::new();
        assert!(registry.get_versions("test").is_none());
    }

    #[test]
    fn test_record_and_retrieve_version() {
        let mut registry = SchemaVersionRegistry::new();
        registry.record_version("users".to_string(), 0, 100);

        let versions = registry.get_versions("users").unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].version, 0);
        assert_eq!(versions[0].timestamp, 100);
    }

    #[test]
    fn test_multiple_versions() {
        let mut registry = SchemaVersionRegistry::new();
        registry.record_version("users".to_string(), 0, 100);
        registry.record_version("users".to_string(), 1, 200);
        registry.record_version("users".to_string(), 2, 300);

        let versions = registry.get_versions("users").unwrap();
        assert_eq!(versions.len(), 3);
        assert_eq!(versions[0].version, 0);
        assert_eq!(versions[2].version, 2);
    }

    #[test]
    fn test_latest_version() {
        let mut registry = SchemaVersionRegistry::new();
        registry.record_version("users".to_string(), 0, 100);
        registry.record_version("users".to_string(), 1, 200);

        assert_eq!(registry.latest_version("users"), Some(1));
        assert_eq!(registry.latest_version("nonexistent"), None);
    }

    #[test]
    fn test_compatibility_check() {
        let registry = SchemaVersionRegistry::new();

        // Same version is compatible
        assert!(registry.is_compatible("users", 0, 0));

        // Incrementing by 1 is compatible
        assert!(registry.is_compatible("users", 0, 1));
        assert!(registry.is_compatible("users", 5, 6));

        // Jumping versions is not compatible
        assert!(!registry.is_compatible("users", 0, 2));
        assert!(!registry.is_compatible("users", 5, 3));
    }

    #[test]
    fn test_multiple_modules() {
        let mut registry = SchemaVersionRegistry::new();
        registry.record_version("users".to_string(), 0, 100);
        registry.record_version("posts".to_string(), 0, 150);
        registry.record_version("users".to_string(), 1, 200);

        assert_eq!(registry.latest_version("users"), Some(1));
        assert_eq!(registry.latest_version("posts"), Some(0));

        let user_versions = registry.get_versions("users").unwrap();
        assert_eq!(user_versions.len(), 2);
    }
}
