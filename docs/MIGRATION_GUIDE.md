# Schema Migration Guide

This guide covers how to safely evolve your table schemas while maintaining data consistency and backward compatibility.

## Overview

Schema migrations in Interstice allow you to:

- Add new columns to tables
- Change column types (with compatibility rules)
- Rename tables and columns
- Record schema changes for recovery and auditing
- Verify migration compatibility with existing data

## Core Concepts

### Schema Versions

Each table schema has a version number:

```rust
use interstice_core::persistence::SchemaVersionRegistry;

let mut registry = SchemaVersionRegistry::new();

// Record a schema version
registry.record_version("users".to_string(), 0, 1234567890);

// Later, check compatibility between versions
let compatible = registry.is_compatible("users", 0, 1);
```

### Migration Records

Migrations are tracked for audit and recovery:

```rust
use interstice_core::persistence::{TableMigration, MigrationRegistry, MigrationRecord};

let mut migrations = MigrationRegistry::new();

let migration = TableMigration::new(
    "add_email".to_string(),
    "users".to_string(),
    0,  // from_version
    1,  // to_version
);

migrations.register("users", migration)?;
```

## Simple Migrations

### Pattern 1: Adding a New Column

The safest migration type - adding an optional field:

```rust
use interstice_core::persistence::TableMigration;
use interstice_abi::IntersticeValue;

let migration = TableMigration::new(
    "add_email_column".to_string(),
    "users".to_string(),
    0,
    1,
);

// Migration rule: For each existing row,
// add a NULL/default value for the new column
// This is backward compatible - old rows still work
```

**Implementation:**

```rust
pub fn migrate_add_email(table: &mut Table, version: u32) -> Result<(), String> {
    if version == 0 {
        // Add new column schema to table
        let new_schema = TableSchema {
            name: table.schema.name.clone(),
            visibility: table.schema.visibility.clone(),
            primary_key: table.schema.primary_key.clone(),
            entries: {
                let mut entries = table.schema.entries.clone();
                entries.push(EntrySchema {
                    name: "email".to_string(),
                    value_type: IntersticeType::String,
                });
                entries
            },
        };

        // Update all existing rows to include empty email
        for row in &mut table.data {
            row.entries.push(IntersticeValue::String(String::new()));
        }

        // Update schema
        table.schema = new_schema;
    }
    Ok(())
}
```

### Pattern 2: Renaming a Column

Rename while preserving data:

```rust
pub fn migrate_rename_column(table: &mut Table) -> Result<(), String> {
    // Find column index
    let email_idx = table.schema.entries
        .iter()
        .position(|e| e.name == "email")
        .ok_or("Column not found")?;

    // Rename in schema
    table.schema.entries[email_idx].name = "user_email".to_string();

    // Data automatically stays with renamed column
    Ok(())
}
```

### Pattern 3: Type Conversion

Change column types (with validation):

```rust
use interstice_abi::{IntersticeType, IntersticeValue};

pub fn migrate_age_to_u32(table: &mut Table) -> Result<(), String> {
    let age_idx = table.schema.entries
        .iter()
        .position(|e| e.name == "age")
        .ok_or("Column not found")?;

    // Validate conversion is safe
    for row in &table.data {
        match &row.entries[age_idx] {
            IntersticeValue::U32(_) => {}
            IntersticeValue::U64(v) if *v < 150 => {
                // Safe to convert
            }
            _ => return Err("Cannot safely convert age column".to_string()),
        }
    }

    // Update schema
    table.schema.entries[age_idx].value_type = IntersticeType::U32;

    // Convert values
    for row in &mut table.data {
        if let IntersticeValue::U64(v) = row.entries[age_idx] {
            row.entries[age_idx] = IntersticeValue::U32(v as u32);
        }
    }

    Ok(())
}
```

## Complex Migrations

### Pattern 4: Data Transformation

Complex transformations during migration:

```rust
pub fn migrate_normalize_emails(table: &mut Table) -> Result<(), String> {
    let email_idx = table.schema.entries
        .iter()
        .position(|e| e.name == "email")
        .ok_or("Column not found")?;

    for row in &mut table.data {
        if let IntersticeValue::String(email) = &mut row.entries[email_idx] {
            // Normalize: trim and lowercase
            *email = email.trim().to_lowercase();
        }
    }

    Ok(())
}
```

### Pattern 5: Column Splitting

Split one column into multiple:

```rust
pub fn migrate_split_name(table: &mut Table) -> Result<(), String> {
    let name_idx = table.schema.entries
        .iter()
        .position(|e| e.name == "full_name")
        .ok_or("Column not found")?;

    // Remove full_name column
    table.schema.entries.remove(name_idx);

    // Add first_name and last_name columns
    table.schema.entries.push(EntrySchema {
        name: "first_name".to_string(),
        value_type: IntersticeType::String,
    });
    table.schema.entries.push(EntrySchema {
        name: "last_name".to_string(),
        value_type: IntersticeType::String,
    });

    // Transform each row
    for row in &mut table.data {
        if let IntersticeValue::String(full_name) = &row.entries[name_idx] {
            let parts: Vec<&str> = full_name.split_whitespace().collect();
            let first = parts.get(0).unwrap_or(&"").to_string();
            let last = parts.get(1).unwrap_or(&"").to_string();

            row.entries.remove(name_idx);
            row.entries.push(IntersticeValue::String(first));
            row.entries.push(IntersticeValue::String(last));
        }
    }

    Ok(())
}
```

## Migration Workflow

### 1. Plan the Migration

```rust
use interstice_core::persistence::TableMigration;

let migration = TableMigration::new(
    "add_timestamps".to_string(),
    "orders".to_string(),
    1,  // current version
    2,  // new version
);

// Verify migration applies
assert!(migration.applies_for("orders", 1, 2));
```

### 2. Implement Migration Logic

```rust
pub fn apply_migration(table: &mut Table, migration_name: &str, version: u32) -> Result<(), String> {
    match migration_name {
        "add_timestamps" => migrate_add_timestamps(table, version),
        "split_name" => migrate_split_name(table),
        _ => Err(format!("Unknown migration: {}", migration_name)),
    }
}
```

### 3. Test with Data

```rust
#[cfg(test)]
mod migration_tests {
    use super::*;

    #[test]
    fn test_migrate_add_timestamps() {
        let mut schema = create_test_schema();
        let mut table = Table::new(schema);

        // Add test data
        let row = Row {
            primary_key: IntersticeValue::U64(1),
            entries: vec![IntersticeValue::String("John".to_string())],
        };
        table.insert_row(row).unwrap();

        // Apply migration
        migrate_add_timestamps(&mut table, 1).unwrap();

        // Verify
        assert_eq!(table.schema.entries.len(), 3); // original + 2 timestamp cols
        assert_eq!(table.data[0].entries.len(), 3);
    }

    #[test]
    fn test_migrate_with_empty_table() {
        let schema = create_test_schema();
        let mut table = Table::new(schema);

        // Should work even with no data
        migrate_add_timestamps(&mut table, 1).unwrap();
        assert_eq!(table.len(), 0);
    }
}
```

### 4. Deploy Migration

```rust
use interstice_core::persistence::MigrationRegistry;

let mut registry = MigrationRegistry::new();

// During startup, check for pending migrations
let pending = registry.pending_migrations("users");

for (name, migration) in pending {
    apply_migration(&mut table, &name, migration.from_version)?;
}
```

## Backward Compatibility Rules

### Safe Migrations ✅

- **Adding columns**: Always safe (use defaults for existing rows)
- **Renaming columns**: Safe (data moves with column)
- **Adding indexes**: Safe (indexes optional)
- **Making fields optional**: Safe
- **Expanding types** (U32 → U64): Safe

### Unsafe Migrations ❌

- **Removing columns**: Data loss
- **Making fields required**: Fails for existing NULL rows
- **Shrinking types** (U64 → U32): May overflow
- **Incompatible type changes** (String → U64): Conversion may fail

### Validation Pattern

```rust
pub fn validate_migration_safe(
    old_schema: &TableSchema,
    new_schema: &TableSchema,
    data: &[Row],
) -> Result<(), String> {
    // Check each column
    for (old_entry, new_entry) in old_schema.entries.iter().zip(&new_schema.entries) {
        // Type conversions must be safe
        if !can_convert(&old_entry.value_type, &new_entry.value_type) {
            return Err(format!(
                "Cannot convert {} from {:?} to {:?}",
                old_entry.name, old_entry.value_type, new_entry.value_type
            ));
        }

        // Validate all existing data
        for row in data {
            // ... conversion validation ...
        }
    }

    Ok(())
}
```

## Deployment Best Practices

### 1. Blue-Green Migrations

Deploy migration to a replica first:

```bash
# 1. Stop replica writes
# 2. Apply migration to replica
cargo run --bin migration-runner -- apply-migration orders v2

# 3. Validate replica state
cargo run --bin validate-log

# 4. If OK, promote replica to primary
# 5. Apply to original primary (rolling update)
```

### 2. Gradual Rollout

For large tables, migrate in chunks:

```rust
pub fn migrate_in_batches(
    table: &mut Table,
    batch_size: usize,
    migration_fn: fn(&mut Row) -> Result<(), String>,
) -> Result<(), String> {
    for batch in table.data.chunks_mut(batch_size) {
        for row in batch {
            migration_fn(row)?;
        }
        // Checkpoint after each batch
    }
    Ok(())
}
```

### 3. Rollback Planning

Always have a rollback strategy:

```rust
pub fn rollback_migration(table: &mut Table, to_version: u32) -> Result<(), String> {
    if to_version == 1 {
        // Remove added columns
        table.schema.entries.remove_last();
        for row in &mut table.data {
            row.entries.remove_last();
        }
    }
    Ok(())
}
```

## Example: Complete Migration

```rust
pub struct MigrationPlan {
    name: String,
    from_version: u32,
    to_version: u32,
    description: String,
}

impl MigrationPlan {
    pub fn add_user_emails() -> Self {
        Self {
            name: "add_user_emails".to_string(),
            from_version: 1,
            to_version: 2,
            description: "Add email column to users table".to_string(),
        }
    }

    pub fn apply(&self, table: &mut Table) -> Result<(), String> {
        // Validate current version
        let current_version = table.schema_version();
        if current_version != self.from_version {
            return Err(format!(
                "Schema version mismatch: expected {}, got {}",
                self.from_version, current_version
            ));
        }

        // Add email column to schema
        table.schema.entries.push(EntrySchema {
            name: "email".to_string(),
            value_type: IntersticeType::String,
        });

        // Add email to all existing rows
        for row in &mut table.data {
            row.entries.push(IntersticeValue::String(String::new()));
        }

        // Update version
        table.set_schema_version(self.to_version);

        Ok(())
    }
}

// Usage
#[test]
fn test_migration() {
    let mut table = create_test_table();
    let migration = MigrationPlan::add_user_emails();

    migration.apply(&mut table).unwrap();

    assert_eq!(table.schema_version(), 2);
    assert!(table.schema.entries.iter().any(|e| e.name == "email"));
}
```

## Summary

| Pattern             | Complexity | Risk   | Rollback      |
| ------------------- | ---------- | ------ | ------------- |
| Add column          | Low        | Low    | Remove column |
| Rename column       | Low        | Low    | Rename back   |
| Split column        | Medium     | Medium | Merge back    |
| Type conversion     | High       | High   | Convert back  |
| Data transformation | High       | High   | Run inverse   |

**Key Rule:** Always make migrations reversible. Record version history for safe rollback.

## Further Reading

- [Schema Versioning](../crates/interstice-core/src/persistence/schema_versioning.rs)
- [Migration Registry](../crates/interstice-core/src/persistence/migration.rs)
- [Recovery Mode](RECOVERY_MODE.md)
