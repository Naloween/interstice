# V1.0 Implementation Quick Start

**Goal:** Get started implementing V1.0 features efficiently.

---

## Before You Start

1. **Read the Plan:** Review [V1.0_IMPLEMENTATION_PLAN.md](./V1.0_IMPLEMENTATION_PLAN.md)
2. **Understand Current State:** Review [ArchitectureOverview.md](./ArchitectureOverview.md)
3. **Check Dependencies:** Ensure Rust toolchain is up-to-date
   ```bash
   rustup update
   cargo build
   cargo test
   ```

---

## Phase 1: Persistence & Durability (Start Here)

### Why Phase 1 First?

- **Foundation:** Everything else depends on durable state
- **Risk Mitigation:** Address the hardest problem early
- **Measurable:** Easy to test (replay engine)

### Getting Started: Transaction Log (1.1)

#### Step 1: Design Review

- **File:** Decide on binary format vs. JSON
  - **Recommended:** Binary (smaller, faster, versioned header)
  - Structure: `[version: u8][type: u8][module_id: u64][table_id: u64][row_len: u32][row_data: bytes][checksum: u32]`
- **Location:** Create `crates/interstice-core/src/persistence/` directory

#### Step 2: Implement TransactionLog

```bash
# Create new module
touch crates/interstice-core/src/persistence/mod.rs
touch crates/interstice-core/src/persistence/transaction_log.rs
```

**Basic structure to implement:**

```rust
pub struct TransactionLog {
    file: File,
    buffer: Vec<u8>,
    path: PathBuf,
}

impl TransactionLog {
    pub fn new(path: PathBuf) -> io::Result<Self> { /* ... */ }
    pub fn append(&mut self, tx: Transaction) -> io::Result<()> { /* ... */ }
    pub fn read_all(&self) -> io::Result<Vec<Transaction>> { /* ... */ }
}
```

**Tests to write:**

- Create log file
- Append transaction
- Read back transaction
- Verify checksum
- Handle concurrent appends (mutex)

#### Step 3: Integrate with Runtime

- Modify `interstice-core/src/lib.rs` to include persistence module
- Add to `Runtime` struct: `transaction_log: Option<TransactionLog>`
- Wrap mutations in a `fn commit_transaction(&mut self, tx: Transaction)`

#### Step 4: Test It

```bash
cargo test --package interstice-core persistence::tests
```

#### Estimated Time: 2-3 days

---

### Next: Replay Engine (1.3)

Once TransactionLog is solid:

1. **Create ReplayEngine struct**

   ```rust
   pub struct ReplayEngine {
       log: TransactionLog,
       modules: HashMap<ModuleId, ModuleState>,
   }
   ```

2. **Implement replay logic**
   - Read log sequentially
   - Apply mutations directly to tables
   - Skip all reducer execution
   - Skip subscriptions

3. **Integrate with startup**

   ```rust
   // In Runtime::new() or similar
   if log_exists {
       let engine = ReplayEngine::new(log_path);
       engine.replay_into(&mut self)?;
   }
   ```

4. **Test with a scenario:**
   - Run a reducer that mutates 3 tables
   - Shut down runtime (savestate)
   - Restart runtime
   - Verify tables have same state
   - Verify reducer didn't re-run

---

## Key Development Practices

### 1. Incremental Integration

- Don't build everything then integrate
- After each sub-feature, integrate and test
- Use feature flags if needed: `--features persistence`

### 2. Example Modules as Tests

- Create a test module in `modules/test_persistence/`
- Use it to validate end-to-end behavior
- Keep it simple: insert rows, mutate state, restart

### 3. Maintain Progress Tracker

- Update [V1.0_PROGRESS.md](./V1.0_PROGRESS.md) as you complete tasks
- Keeps momentum and clarity
- Useful if someone else joins

### 4. Documentation During Development

- Add doc comments as you code
- Update architecture docs if design changes
- Keep README in sync

### 5. Test-Driven Approach

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_persistence() {
        let tmpdir = tempfile::tempdir().unwrap();
        let log_path = tmpdir.path().join("test.log");

        // Write
        let mut log = TransactionLog::new(log_path.clone()).unwrap();
        log.append(test_transaction()).unwrap();

        // Read
        let read_log = TransactionLog::new(log_path).unwrap();
        let txs = read_log.read_all().unwrap();
        assert_eq!(txs.len(), 1);
    }
}
```

---

## Dependency Checklist

Before implementing each phase, verify dependencies:

### Phase 1 Dependencies

- [x] Core Runtime exists
- [ ] Module loading works
- [ ] Reducers execute

### Phase 2 Dependencies

- [ ] Phase 1 complete
- [ ] Persistence stable

### Phase 3 Dependencies

- [ ] Phase 2 complete
- [ ] Storage layer stable

### Phase 4 Dependencies

- [ ] Phases 1-3 complete
- [ ] Codebase stabilized

---

## Debugging Tips

### Log Replay Issues

```bash
# Enable debug logs
RUST_LOG=debug cargo run -- --replay-log test.log

# Export log as JSON
cargo run -- log-dump test.log --format json > log.json
```

### Test Determinism

```bash
# Record execution
cargo test -- --nocapture 2> trace1.log

# Run again, diff traces
cargo test -- --nocapture 2> trace2.log
diff trace1.log trace2.log
```

---

## Building the CLI Tools

Each Phase 4 CLI tool follows a pattern:

```rust
// In crates/interstice-cli/src/commands/mod.rs
pub mod schema;
pub mod validate;
pub mod log_inspect;
pub mod replay_check;

// Run with:
// cargo run --bin interstice -- schema path/to/module.wasm
```

---

## Common Mistakes to Avoid

1. **Over-engineering:** Start simple, add features as needed
2. **Skipping tests:** Persistence is not forgiving of bugs
3. **Ignoring determinism:** Test replay early and often
4. **Coupling:** Keep log format separate from runtime state
5. **Forgetting error cases:** What if log is corrupted? Partially written?

---

## Parallel Work Opportunities

While working on Phase 1, you can parallel-path:

- **Design Phase 2 index interface** (document, don't code yet)
- **Expand example modules** (use for testing)
- **Write Phase 2 tests** (mock the index behavior)
- **Review and improve existing SDK** (small improvements)

---

## When to Call it "Done" for a Phase

A phase is complete when:

1. ✅ All tasks are implemented
2. ✅ Unit tests pass (80%+ coverage)
3. ✅ Integration tests with examples pass
4. ✅ No known bugs or regressions
5. ✅ Documentation is updated
6. ✅ Progress tracker is marked complete

---

## Next Steps After Reading This

1. **Start Phase 1.1:**
   - Decide on transaction log binary format
   - Create `persistence/` module
   - Implement `TransactionLog` struct

2. **Set a checkpoint:**
   - Goal: TransactionLog appending + reading works
   - Timeline: 2-3 days
   - Test: Create, append, restart, read

3. **Update progress tracker:**
   - Mark 1.1 "In Progress"
   - Commit progress to git regularly

Good luck! 🚀
