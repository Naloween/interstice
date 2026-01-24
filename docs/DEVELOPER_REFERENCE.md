# Interstice V1.0 Developer Reference

Quick reference for common development tasks during V1.0 implementation.

---

## Project Structure

```
interstice/
├── crates/                  # Core Rust libraries
│   ├── interstice-core/     # Main runtime (where most work happens)
│   ├── interstice-sdk/      # Module SDK (SDK ergonomics improvements)
│   ├── interstice-sdk-core/ # SDK base types
│   ├── interstice-sdk-macros/# Proc macros (#[reducer], #[table], etc)
│   └── interstice-abi/      # ABI definitions (stable)
├── modules/                 # Example modules
│   ├── hello/               # Basic example
│   ├── graphics/            # Graphics subsystem
│   └── caller/              # Multi-module example
├── examples/                # Standalone examples
├── docs/                    # Documentation (you are here)
└── Cargo.toml              # Workspace manifest
```

---

## File Organization During Implementation

### Adding Features to Core Runtime

```
crates/interstice-core/src/
├── lib.rs                  # Main entry point
├── runtime.rs              # Runtime struct
├── module_loader.rs        # WASM loading
├── table.rs                # Table operations
├── reducer.rs              # Reducer execution
├── subscription.rs         # Subscription tracking
├── capability.rs           # Capability enforcement
├── persistence/            # ← NEW: Phase 1 additions
│   ├── mod.rs              # Module exports
│   ├── transaction_log.rs  # Log implementation
│   ├── replay_engine.rs    # Replay logic
│   └── tests.rs            # Integration tests
└── storage/                # ← Phase 2 additions
    ├── mod.rs
    ├── index.rs
    └── migrations.rs
```

### Adding SDK Improvements

```
crates/interstice-sdk/src/
├── lib.rs                  # Main exports
├── context.rs              # Reducer context
├── table_handle.rs         # ← Phase 3: Typed tables
├── reducer_call.rs         # ← Phase 3: Typed calls
└── macros.rs               # Macro re-exports
```

### Adding CLI Tools

```
crates/interstice-cli/     # ← NEW in Phase 4
├── src/
│   ├── main.rs             # CLI entry point
│   ├── commands/
│   │   ├── schema.rs       # schema inspect
│   │   ├── validate.rs     # validate module
│   │   ├── log_inspect.rs  # log-inspect
│   │   ├── replay_check.rs # replay-check
│   │   └── ...
│   └── cli.rs              # Arg parsing
└── Cargo.toml
```

---

## Common Commands

### Build Everything

```bash
cargo build
# or with specific packages:
cargo build -p interstice-core
cargo build -p interstice-sdk
```

### Run Tests

```bash
# All tests
cargo test

# Specific crate
cargo test -p interstice-core

# Specific test
cargo test test_log_persistence

# With output
cargo test -- --nocapture
```

### Build Examples

```bash
cargo build --example hello
# Then run the compiled binary
```

### Check Code

```bash
# Type check without building (faster)
cargo check

# Format code
cargo fmt

# Lint
cargo clippy
```

### Clean Build

```bash
cargo clean
cargo build
```

---

## Testing Patterns

### Unit Test Template

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_name() {
        // Arrange
        let mut log = TransactionLog::new(path).unwrap();

        // Act
        log.append(transaction).unwrap();

        // Assert
        assert_eq!(log.len(), 1);
    }
}
```

### Integration Test Template

```rust
// tests/persistence_integration.rs
#[test]
fn test_persistence_roundtrip() {
    let tempdir = tempfile::tempdir().unwrap();

    // Start runtime with log
    let mut rt = Runtime::new_with_log(tempdir.path()).unwrap();

    // Load module and call reducer
    let module_id = rt.load_module("test_module.wasm").unwrap();
    rt.call_reducer(module_id, "test_reducer", args).unwrap();

    // Verify state before shutdown
    let state_before = rt.query_table(table_id, filter).unwrap();

    // Restart
    drop(rt);
    let mut rt = Runtime::new_with_log(tempdir.path()).unwrap();

    // Verify state after restart
    let state_after = rt.query_table(table_id, filter).unwrap();
    assert_eq!(state_before, state_after);
}
```

### Property-Based Testing

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn test_replay_idempotent(
        txs in prop::collection::vec(valid_transaction(), 1..100)
    ) {
        let mut log1 = TransactionLog::new(path1).unwrap();
        let mut log2 = TransactionLog::new(path2).unwrap();

        for tx in &txs {
            log1.append(tx.clone()).unwrap();
            log2.append(tx.clone()).unwrap();
        }

        let replay1 = ReplayEngine::new(log1).replay().unwrap();
        let replay2 = ReplayEngine::new(log2).replay().unwrap();

        assert_eq!(replay1, replay2);
    }
}
```

---

## Debugging

### Enable Debug Logging

```bash
RUST_LOG=debug cargo run --example hello
RUST_LOG=interstice_core=trace cargo test
```

### Add println Debugging

```rust
// Temporary debugging
eprintln!("DEBUG: transaction = {:?}", tx);

// Better: use log/tracing
log::debug!("Processing transaction: {:?}", tx);
tracing::debug!(module_id = ?module_id, "Calling reducer");
```

### GDB Debugging

```bash
# Compile with debug symbols
RUST_BACKTRACE=full cargo test -- --nocapture --test-threads=1

# Or use rust-gdb
rust-gdb --args ./target/debug/interstice
```

### Examine Generated Code

```bash
# See what macros generate
cargo expand --package interstice-sdk --lib
```

---

## Documentation Standards

### Code Comments

````rust
/// Appends a transaction to the log.
///
/// # Arguments
/// * `tx` - The transaction to append
///
/// # Returns
/// Ok(()) if successful, Err if I/O fails
///
/// # Example
/// ```
/// let mut log = TransactionLog::new(path)?;
/// log.append(tx)?;
/// ```
pub fn append(&mut self, tx: Transaction) -> io::Result<()> {
    // Implementation comment if non-obvious
    // Prefer clear code over comments
}
````

### Architecture Docs

- Update relevant section in ArchitectureOverview.md
- Add diagrams for complex systems
- Link from code to docs

### User Docs

- Examples in docs/examples/
- Copy-paste ready code snippets
- Include expected output

---

## Performance Considerations

### Profiling

```bash
# Flamegraph
cargo install flamegraph
cargo flamegraph --example hello

# Perf stat
perf stat -r 5 cargo test persistence
```

### Optimization Checklist

- [ ] Profile before optimizing
- [ ] Identify bottlenecks (80/20 rule)
- [ ] Benchmark before/after
- [ ] Don't sacrifice clarity for performance
- [ ] Document performance-critical code

### Common Optimizations

```rust
// Prefer iterators over collecting
// ❌ log.transactions().collect()
// ✅ log.transactions()

// Avoid cloning large structures
// ❌ let mut copy = state.clone();
// ✅ let mut_ref = &mut state;

// Use references in loops
// ❌ for tx in transactions { process(&tx); }
// ✅ for tx in &transactions { process(tx); }
```

---

## Cargo Workspace Commands

### Add a New Crate

```bash
cargo new crates/my-crate --lib
# Edit Cargo.toml to add dependencies
# Add to root Cargo.toml: members = ["crates/my-crate", ...]
```

### Add Dependencies

```bash
cargo add -p interstice-core serde
# or edit Cargo.toml directly
```

### Publish Version (after V1.0)

```bash
# Update version in Cargo.toml files
# Commit and tag
git tag v1.0.0
cargo publish
```

---

## Git Workflow

### Before Starting a Task

```bash
git checkout main
git pull origin main
git checkout -b feat/persistence-phase-1
```

### After Completing a Task

```bash
git add docs/V1.0_PROGRESS.md crates/...
git commit -m "feat: implement transaction log persistence

- Add TransactionLog struct with append/read
- Integrate with Runtime for mutation tracking
- Add log rotation and integrity checks

Closes #42"
```

### Keeping Track of Changes

```bash
git diff                    # Unstaged changes
git diff --cached           # Staged changes
git status                  # Overview
```

---

## Useful Crates (Already in Use)

- **wasmtime** - WASM runtime
- **serde** - Serialization
- **parking_lot** - Better Mutex
- **tracing** - Logging/tracing
- **proptest** - Property testing
- **tempfile** - Temporary files for testing

### Consider Adding

- **bincode** - Binary serialization (for transaction log)
- **crc** - Checksums
- **nom** - Binary parsing
- **indicatif** - Progress bars for CLI

---

## Troubleshooting

### "Cargo lock is outdated"

```bash
cargo update
```

### Tests hang

```bash
# Check for deadlocks
RUST_BACKTRACE=1 timeout 30 cargo test -- --test-threads=1
```

### Compiler error in macros

```bash
# Expand macros to see generated code
cargo expand --package interstice-sdk-macros
```

### Module loading fails

```bash
# Verify WASM binary
cargo run --bin interstice -- validate module.wasm
```

---

## Phase-Specific Tips

### Phase 1: Persistence

- Keep log format simple initially
- Write extensive replay tests
- Test corruption recovery early
- Don't over-engineer snapshots

### Phase 2: Storage

- Benchmark before/after adding indexes
- Test with realistic data sizes (1M+ rows)
- Document index overhead

### Phase 3: SDK

- Run examples frequently—catch ergonomics issues early
- Get feedback from potential module authors
- Keep breaking changes to one release

### Phase 4: Tooling

- CLI should be fast (< 100ms startup)
- Visualizations should be production-grade
- Consider web UI later (post-V1.0)

---

## Resources

- **Rust Book:** https://doc.rust-lang.org/book/
- **WASM Spec:** https://webassembly.org/
- **Wasmtime Docs:** https://docs.wasmtime.dev/
- **This Project:** docs/ folder

---

## Getting Help

1. **Compile error?** → Expand macros, check types
2. **Test failing?** → Add debug output, run with `--nocapture`
3. **Design question?** → Review ArchitectureOverview.md
4. **Performance issue?** → Profile with flamegraph
5. **Stuck on Phase?** → Review examples/, ask for design review
