# Recovery Mode Documentation

This guide covers how to recover from various failure scenarios when using Interstice with transaction logging enabled.

## Overview

Interstice provides durability through append-only transaction logs. When a process crashes or is forcefully terminated, the state can be completely recovered by replaying the transaction log on startup.

## Automatic Recovery (Recommended)

### Setup

Use the `with_persistence_and_auto_replay` method for production systems:

```rust
use interstice_core::persistence::PersistenceConfig;
use interstice_core::runtime::Runtime;
use std::path::PathBuf;

let config = PersistenceConfig {
    enabled: true,
    log_dir: PathBuf::from("./interstice_logs"),
    sync_on_append: true,
};

// This automatically replays the log if it exists
let runtime = Runtime::with_persistence_and_auto_replay(config)?;
```

**Behavior:**

1. Runtime initializes with persistence enabled
2. Checks for existing transaction log at `./interstice_logs/txn.log`
3. If log exists, automatically replays all transactions
4. State is fully restored before application starts processing new requests
5. No data loss occurs

## Manual Recovery

### When to Use Manual Recovery

- Diagnostic purposes
- Custom recovery procedures
- Testing recovery flows

### Setup and Replay

```rust
use interstice_core::persistence::{PersistenceConfig, ReplayEngine};
use interstice_core::runtime::Runtime;
use std::path::PathBuf;

let config = PersistenceConfig {
    enabled: true,
    log_dir: PathBuf::from("./interstice_logs"),
    sync_on_append: true,
};

let mut runtime = Runtime::with_persistence(config)?;

// Manually trigger replay
let replayed_count = runtime.replay_from_log()?;
println!("Successfully replayed {} transactions", replayed_count);
```

## Corruption Recovery

### Detecting Corruption

Use the log validation tool to check for corrupted records:

```rust
use interstice_core::persistence::LogValidator;

let result = LogValidator::validate("./interstice_logs/txn.log")?;

if !result.is_valid() {
    eprintln!("Log corruption detected!");
    eprintln!("Valid transactions: {}", result.valid_transactions);
    eprintln!("Invalid transactions: {}", result.invalid_transactions);
    eprintln!("Errors: {:?}", result.errors);
}
```

### Recovery Strategies

#### Strategy 1: Truncate to Last Valid Record

If the log is corrupted at the end:

```rust
use interstice_core::persistence::LogValidator;
use std::fs::OpenOptions;

// Validate to find where corruption starts
let result = LogValidator::validate("./interstice_logs/txn.log")?;
let last_valid_bytes = result.last_valid_byte_offset;

// Truncate file to last valid record
let file = OpenOptions::new()
    .write(true)
    .open("./interstice_logs/txn.log")?;
file.set_len(last_valid_bytes as u64)?;

// Now replay will work with clean data
```

#### Strategy 2: Skip Corrupted Transactions

For applications that can tolerate data loss of in-flight transactions:

```rust
use interstice_core::persistence::ReplayEngine;
use interstice_core::runtime::Runtime;

let mut runtime = Runtime::with_persistence(config)?;

// Replay will skip corrupted records and continue
let replayed_count = runtime.replay_from_log()?;

// Data up to the corruption point is restored
// Transactions after corruption are lost (safest approach)
```

#### Strategy 3: Manual Log Inspection

Inspect the log to understand what corrupted:

```rust
use interstice_core::persistence::LogValidator;

let info = LogValidator::inspect("./interstice_logs/txn.log")?;

println!("Transaction count: {}", info.transaction_count);
println!("Modules: {:?}", info.modules);
println!("Tables affected: {:?}", info.tables);
println!("Last valid timestamp: {}", info.last_timestamp);
```

## Log Rotation and Cleanup

### Automatic Rotation

Interstice automatically rotates transaction logs when they exceed the configured size:

```rust
use interstice_core::persistence::log_rotation::RotationConfig;

let rotation_config = RotationConfig {
    max_log_size: 100 * 1024 * 1024,  // 100MB
    max_rotated_logs: 10,              // Keep 10 old logs
};

// TransactionLog uses this config automatically
let log = TransactionLog::with_rotation("txn.log", rotation_config)?;
```

**Rotation behavior:**

- `txn.log` → `txn.log.0` (when size exceeded)
- `txn.log.0` → `txn.log.1` (on next rotation)
- Old logs beyond `max_rotated_logs` are automatically deleted

### Manual Cleanup

To manually clean up rotated logs:

```rust
use interstice_core::persistence::LogRotator;
use std::fs;

let rotator = LogRotator::new(RotationConfig::default());
let logs = rotator.list_rotated_logs(Path::new("txn.log"))?;

for log_path in logs {
    println!("Found rotated log: {}", log_path.display());
    // Optionally delete: fs::remove_file(log_path)?;
}
```

## Replica Synchronization

For high-availability setups with multiple replicas:

### Step 1: Flush and Copy Log

```bash
# On primary, ensure all data is synced
# Copy the log to replica location
cp interstice_logs/txn.log replica_logs/txn.log
cp interstice_logs/txn.log.0 replica_logs/txn.log.0 2>/dev/null || true
```

### Step 2: Replica Recovery

```rust
// Replica initialization
let config = PersistenceConfig {
    enabled: true,
    log_dir: PathBuf::from("./replica_logs"),
    sync_on_append: true,
};

let runtime = Runtime::with_persistence_and_auto_replay(config)?;
// Replica now has exact same state as primary
```

## Failure Scenarios

### Scenario 1: Gradual Corruption (Bad Sector)

**Symptom:** Log validation reports checksum mismatches
**Recovery:**

1. Validate log to find where corruption starts
2. Truncate to last valid record
3. Replay truncated log
4. Applications continue with pre-corruption state

### Scenario 2: Sudden Crash Mid-Write

**Symptom:** Last few transactions missing
**Recovery:**

1. Use auto-replay on startup
2. Log is append-only, so partial writes don't corrupt previous data
3. Failed transaction is simply not present in log
4. System continues from last complete transaction

### Scenario 3: Disk Full

**Symptom:** Write failures in transaction log
**Recovery:**

1. Free up disk space
2. Existing log is untouched
3. Restart application
4. Auto-replay restores state
5. Normal operation resumes

### Scenario 4: File System Metadata Corruption

**Symptom:** Log file exists but can't be opened/read
**Recovery:**

1. Make backup: `cp txn.log txn.log.backup`
2. Clear/remove the corrupted file
3. Restart application
4. Depending on backup strategy, either:
   - Start fresh (data loss)
   - Restore from backup and replay
   - Use replica as source of truth

## Best Practices

### 1. Regular Validation

Run periodic log validation in production:

```rust
use std::thread;
use std::time::Duration;

// Spawn background validation every hour
let handle = thread::spawn(|| {
    loop {
        thread::sleep(Duration::from_secs(3600));
        match LogValidator::validate("./interstice_logs/txn.log") {
            Ok(result) if result.is_valid() => {
                println!("Log validation: OK ({} transactions)", result.valid_transactions);
            }
            Ok(result) => {
                eprintln!("Log corruption detected: {:?}", result.errors);
                // Alert operators
            }
            Err(e) => {
                eprintln!("Validation error: {}", e);
            }
        }
    }
});
```

### 2. Log Backups

Regular backups protect against disk failure:

```bash
# Daily backup script
#!/bin/bash
BACKUP_DIR=/backups/interstice
TIMESTAMP=$(date +%Y%m%d_%H%M%S)

mkdir -p $BACKUP_DIR
cp interstice_logs/txn.log* $BACKUP_DIR/txn.log_$TIMESTAMP
```

### 3. Monitoring Disk Space

Ensure adequate disk space for log rotation:

```rust
use std::fs;

let metadata = fs::metadata("./interstice_logs")?;
let available = metadata.
remaining space();

if available < 500 * 1024 * 1024 {  // Less than 500MB
    eprintln!("Warning: Low disk space for transaction logs");
    // Alert ops team
}
```

### 4. Schema Version Tracking

Always record schema versions for safe recovery:

```rust
use interstice_core::persistence::SchemaVersionRegistry;

let mut registry = SchemaVersionRegistry::new();
registry.record_version("users".to_string(), 1, SystemTime::now());

// During recovery, check compatibility
let compatible = registry.is_compatible("users", 1, 2);
```

## Summary

| Scenario                 | Automatic | Manual | Risk                      |
| ------------------------ | --------- | ------ | ------------------------- |
| Clean shutdown & restart | ✓         | -      | None                      |
| Process crash            | ✓         | ✓      | None (append-only)        |
| Log corruption at end    | ✗         | ✓      | Fix corruption            |
| Mid-transaction failure  | ✓         | ✓      | Previous transaction safe |
| Disk full                | ✓         | ✓      | Free space required       |
| Disk failure             | ✓\*       | ✓      | Backup required           |

\* Requires replica or backup available

## Further Reading

- [Persistence Architecture](ArchitectureOverview.md#persistence-layer)
- [Transaction Log Format](../crates/interstice-core/src/persistence/transaction_log.rs)
- [Schema Versioning](../crates/interstice-core/src/persistence/schema_versioning.rs)
