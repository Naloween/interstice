// Table schema migration system for safe schema evolution
use std::collections::HashMap;

/// Represents a single migration that transforms table state
#[derive(Clone, Debug)]
pub struct TableMigration {
    pub name: String,
    pub from_version: u32,
    pub to_version: u32,
    pub table_name: String,
    pub description: String,
}

impl TableMigration {
    pub fn new(
        name: String,
        table_name: String,
        from_version: u32,
        to_version: u32,
        description: String,
    ) -> Self {
        TableMigration {
            name,
            from_version,
            to_version,
            table_name,
            description,
        }
    }

    /// Check if this migration applies for the given transition
    pub fn applies(&self, table_name: &str, current_version: u32, target_version: u32) -> bool {
        self.table_name == table_name
            && self.from_version == current_version
            && self.to_version == target_version
    }
}

/// Tracks which migrations have been applied
#[derive(Clone, Debug)]
pub struct MigrationRecord {
    pub migration_name: String,
    pub applied_at: u64, // Logical timestamp
    pub table_name: String,
}

impl MigrationRecord {
    pub fn new(migration_name: String, table_name: String, applied_at: u64) -> Self {
        MigrationRecord {
            migration_name,
            applied_at,
            table_name,
        }
    }
}

/// Registry for managing table migrations
pub struct MigrationRegistry {
    migrations: HashMap<String, TableMigration>,
    applied: Vec<MigrationRecord>,
}

impl MigrationRegistry {
    pub fn new() -> Self {
        MigrationRegistry {
            migrations: HashMap::new(),
            applied: Vec::new(),
        }
    }

    /// Register a migration
    pub fn register(&mut self, migration: TableMigration) -> Result<(), String> {
        if self.migrations.contains_key(&migration.name) {
            return Err(format!("Migration {} already registered", migration.name));
        }
        self.migrations.insert(migration.name.clone(), migration);
        Ok(())
    }

    /// Get migration by name
    pub fn get(&self, name: &str) -> Option<&TableMigration> {
        self.migrations.get(name)
    }

    /// Find migrations that apply for a version transition
    pub fn find_applicable(
        &self,
        table_name: &str,
        from_version: u32,
        to_version: u32,
    ) -> Vec<&TableMigration> {
        self.migrations
            .values()
            .filter(|m| m.table_name == table_name && m.from_version >= from_version && m.to_version <= to_version)
            .collect()
    }

    /// Record a migration as applied
    pub fn record_applied(&mut self, migration_name: String, table_name: String, timestamp: u64) {
        self.applied.push(MigrationRecord::new(migration_name, table_name, timestamp));
    }

    /// Check if a migration has been applied
    pub fn is_applied(&self, migration_name: &str) -> bool {
        self.applied.iter().any(|r| r.migration_name == migration_name)
    }

    /// Get all applied migrations for a table
    pub fn get_applied_for_table(&self, table_name: &str) -> Vec<&MigrationRecord> {
        self.applied
            .iter()
            .filter(|r| r.table_name == table_name)
            .collect()
    }

    /// Get all migrations
    pub fn all_migrations(&self) -> Vec<&TableMigration> {
        self.migrations.values().collect()
    }

    /// Get count of registered migrations
    pub fn migration_count(&self) -> usize {
        self.migrations.len()
    }

    /// Get count of applied migrations
    pub fn applied_count(&self) -> usize {
        self.applied.len()
    }
}

impl Default for MigrationRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migration_creation() {
        let m = TableMigration::new(
            "add_email_column".to_string(),
            "users".to_string(),
            1,
            2,
            "Add email column to users table".to_string(),
        );
        assert_eq!(m.name, "add_email_column");
        assert_eq!(m.from_version, 1);
        assert_eq!(m.to_version, 2);
    }

    #[test]
    fn test_migration_applies() {
        let m = TableMigration::new(
            "add_email_column".to_string(),
            "users".to_string(),
            1,
            2,
            "Add email column".to_string(),
        );
        assert!(m.applies("users", 1, 2));
        assert!(!m.applies("users", 1, 3));
        assert!(!m.applies("accounts", 1, 2));
    }

    #[test]
    fn test_registry_register() {
        let mut registry = MigrationRegistry::new();
        let m = TableMigration::new(
            "migration_1".to_string(),
            "users".to_string(),
            1,
            2,
            "First migration".to_string(),
        );
        assert!(registry.register(m).is_ok());
        assert_eq!(registry.migration_count(), 1);
    }

    #[test]
    fn test_registry_duplicate_registration_fails() {
        let mut registry = MigrationRegistry::new();
        let m1 = TableMigration::new(
            "migration_1".to_string(),
            "users".to_string(),
            1,
            2,
            "First migration".to_string(),
        );
        let m2 = TableMigration::new(
            "migration_1".to_string(),
            "users".to_string(),
            2,
            3,
            "Duplicate name".to_string(),
        );
        assert!(registry.register(m1).is_ok());
        assert!(registry.register(m2).is_err());
    }

    #[test]
    fn test_registry_get_migration() {
        let mut registry = MigrationRegistry::new();
        let m = TableMigration::new(
            "migration_1".to_string(),
            "users".to_string(),
            1,
            2,
            "Test migration".to_string(),
        );
        registry.register(m).ok();
        
        let found = registry.get("migration_1");
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "migration_1");
    }

    #[test]
    fn test_registry_find_applicable() {
        let mut registry = MigrationRegistry::new();
        registry
            .register(TableMigration::new(
                "m1".to_string(),
                "users".to_string(),
                1,
                2,
                "".to_string(),
            ))
            .ok();
        registry
            .register(TableMigration::new(
                "m2".to_string(),
                "users".to_string(),
                2,
                3,
                "".to_string(),
            ))
            .ok();
        registry
            .register(TableMigration::new(
                "m3".to_string(),
                "accounts".to_string(),
                1,
                2,
                "".to_string(),
            ))
            .ok();

        let applicable = registry.find_applicable("users", 1, 3);
        assert_eq!(applicable.len(), 2);
    }

    #[test]
    fn test_migration_record() {
        let record = MigrationRecord::new("m1".to_string(), "users".to_string(), 12345);
        assert_eq!(record.migration_name, "m1");
        assert_eq!(record.table_name, "users");
        assert_eq!(record.applied_at, 12345);
    }

    #[test]
    fn test_registry_record_applied() {
        let mut registry = MigrationRegistry::new();
        registry.record_applied("m1".to_string(), "users".to_string(), 100);
        assert_eq!(registry.applied_count(), 1);
        assert!(registry.is_applied("m1"));
    }

    #[test]
    fn test_registry_is_applied() {
        let mut registry = MigrationRegistry::new();
        registry.record_applied("m1".to_string(), "users".to_string(), 100);
        assert!(registry.is_applied("m1"));
        assert!(!registry.is_applied("m2"));
    }

    #[test]
    fn test_registry_get_applied_for_table() {
        let mut registry = MigrationRegistry::new();
        registry.record_applied("m1".to_string(), "users".to_string(), 100);
        registry.record_applied("m2".to_string(), "users".to_string(), 101);
        registry.record_applied("m3".to_string(), "accounts".to_string(), 102);

        let applied = registry.get_applied_for_table("users");
        assert_eq!(applied.len(), 2);
    }

    #[test]
    fn test_registry_all_migrations() {
        let mut registry = MigrationRegistry::new();
        registry
            .register(TableMigration::new(
                "m1".to_string(),
                "users".to_string(),
                1,
                2,
                "".to_string(),
            ))
            .ok();
        registry
            .register(TableMigration::new(
                "m2".to_string(),
                "users".to_string(),
                2,
                3,
                "".to_string(),
            ))
            .ok();

        assert_eq!(registry.all_migrations().len(), 2);
    }

    #[test]
    fn test_registry_multiple_tables() {
        let mut registry = MigrationRegistry::new();
        registry
            .register(TableMigration::new(
                "users_m1".to_string(),
                "users".to_string(),
                1,
                2,
                "".to_string(),
            ))
            .ok();
        registry
            .register(TableMigration::new(
                "accounts_m1".to_string(),
                "accounts".to_string(),
                1,
                2,
                "".to_string(),
            ))
            .ok();

        assert_eq!(registry.migration_count(), 2);
        
        let users_applied = registry.find_applicable("users", 1, 2);
        assert_eq!(users_applied.len(), 1);
        assert_eq!(users_applied[0].table_name, "users");
    }
}
