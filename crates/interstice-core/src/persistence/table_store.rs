//! Table persistence (DESIGN §3.2, §6.7).
//!
//! - Stateful tables: one file per row, flushed in the background.
//! - Logged tables: an append-only log that is also the table's history.
//!   It is split into segments (`logs/<table>/<first_seq>.log`), with
//!   checkpoints (`snapshots/<table>/<seq>-<tx>-<ts>.snap`) so that restore,
//!   and reads of past states, start from a nearby full copy of the table.

use std::collections::{HashMap, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use interstice_abi::{
    HistoryCommit, HistoryPoint, IndexKey, PersistenceKind, Row, TableSchema, decode, encode,
};
use serde::{Deserialize, Serialize};

use crate::{error::IntersticeError, runtime::table::Table};

const SNAPSHOT_VERSION: u16 = 2;
/// A checkpoint is taken once this many entries, or as many entries as the
/// table had rows at the last checkpoint, were logged since it (DESIGN §6.7).
const CHECKPOINT_MIN_INTERVAL: u64 = 256;

/// A checkpoint to write once the table lock is released.
#[derive(Clone, Debug)]
pub struct SnapshotPlan {
    pub module: String,
    pub table: String,
    pub seq: u64,
    pub tx: u64,
    pub timestamp_ms: u64,
}

/// The transaction a log entry belongs to: `(tx id, timestamp_ms)`.
pub type LogStamp = (u64, u64);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum LogOperation {
    Insert {
        primary_key: IndexKey,
        row: Option<Row>,
    },
    Update {
        primary_key: IndexKey,
        row: Option<Row>,
    },
    Delete {
        primary_key: IndexKey,
    },
    Clear,
    /// Tags a commit as a restore to `to` (DESIGN §6.7); applies nothing.
    Restore {
        to: u64,
    },
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct TableKey {
    module: String,
    table: String,
}

impl WalWriter {
    fn write_pending(&mut self) -> Result<(), IntersticeError> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let result = self.file.write_all(&self.pending);
        self.pending.clear();
        self.dirty = true;
        result.map_err(|err| IntersticeError::Internal(format!("Failed to write log entries: {err}")))
    }
}

impl Drop for WalWriter {
    /// A writer removed from the map (retired, module removed) still
    /// writes what it buffered.
    fn drop(&mut self) {
        let _ = self.write_pending();
    }
}

impl TableKey {
    fn new(module: &str, table: &str) -> Self {
        Self {
            module: module.to_string(),
            table: table.to_string(),
        }
    }
}

#[derive(Debug)]
struct TableState {
    persistence: PersistenceKind,
    next_seq: u64,
    /// First seq of the segment new entries go to.
    segment_start: u64,
    entries_since_checkpoint: u64,
    /// Rows in the table at the last checkpoint.
    rows_at_checkpoint: u64,
    /// `TableSchema::history`: how long the log is kept, in ms.
    history_ms: Option<u64>,
    /// The current segment's path, for `segment_start` (saves building it
    /// on every write).
    segment_path: Option<(u64, PathBuf)>,
}

impl TableState {
    fn new(persistence: PersistenceKind) -> Self {
        Self {
            persistence,
            next_seq: 0,
            segment_start: 0,
            entries_since_checkpoint: 0,
            rows_at_checkpoint: 0,
            history_ms: None,
            segment_path: None,
        }
    }
}

/// Persistent open file handle for async WAL writes.
struct WalWriter {
    file: File,
    /// Entries of the commit in progress, written in one call by
    /// [`TableStore::write_pending`] (DESIGN §3.2).
    pending: Vec<u8>,
    /// Absolute path to this log file (for re-opening a sync fd).
    path: PathBuf,
    /// True when entries have been written but not yet fsynced.
    dirty: bool,
    /// The segment is complete: close it after its next sync.
    retired: bool,
}

/// Key into the dirty-stateful map: (module, table, primary key).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct StatefulRowKey {
    module: String,
    table: String,
    pk: IndexKey,
}

pub struct TableStore {
    modules_root: Option<PathBuf>,
    tables: Mutex<HashMap<TableKey, Arc<Mutex<TableState>>>>,
    /// Open WAL log-file handles keyed by absolute path.
    /// Hot path: write without fsync; background WAL thread fsyncs every 10ms.
    wal_writers: Arc<Mutex<HashMap<PathBuf, WalWriter>>>,
    /// Pending stateful row writes — flushed to disk by background thread every 10ms.
    /// `None` value means the row was deleted.
    /// Bounded by the number of unique rows touched (not by operation count).
    dirty_stateful: Arc<Mutex<HashMap<StatefulRowKey, Option<Row>>>>,
    /// Serialises read-modify-write of `sequences.toml` files.
    sequences: Mutex<()>,
}

/// `modules/<module>/sequences.toml`: the highest id each auto-increment
/// counter may have handed out (DESIGN §3.2).
#[derive(Default, Serialize, Deserialize)]
struct Sequences {
    #[serde(default)]
    ceilings: std::collections::BTreeMap<String, u64>,
}

impl TableStore {
    fn sequences_path(&self, module: &str) -> Option<PathBuf> {
        self.modules_root.as_ref().map(|root| root.join(module).join("sequences.toml"))
    }

    /// The saved ceilings of `module`'s counters, by `table.field`.
    pub fn sequence_ceilings(&self, module: &str) -> HashMap<String, u64> {
        let Some(path) = self.sequences_path(module) else { return HashMap::new() };
        fs::read_to_string(path)
            .ok()
            .and_then(|text| toml::from_str::<Sequences>(&text).ok())
            .map(|s| s.ceilings.into_iter().collect())
            .unwrap_or_default()
    }

    /// Save that `module`'s counter `key` may hand out ids below `ceiling`,
    /// durably, before any of them is used.
    pub fn save_sequence_ceiling(&self, module: &str, key: &str, ceiling: u64) -> Result<(), IntersticeError> {
        let Some(path) = self.sequences_path(module) else { return Ok(()) };
        let _guard = self.sequences.lock();
        let mut sequences = fs::read_to_string(&path)
            .ok()
            .and_then(|text| toml::from_str::<Sequences>(&text).ok())
            .unwrap_or_default();
        sequences.ceilings.insert(key.to_string(), ceiling);
        let text = toml::to_string(&sequences)
            .map_err(|e| IntersticeError::Internal(format!("Failed to encode sequences: {e}")))?;
        if let Some(dir) = path.parent() {
            create_dir(dir)?;
        }
        write_atomically(&path, text.as_bytes())
    }

    pub fn new(root: Option<PathBuf>) -> Self {
        Self {
            modules_root: root,
            sequences: Mutex::new(()),
            tables: Mutex::new(HashMap::new()),
            wal_writers: Arc::new(Mutex::new(HashMap::new())),
            dirty_stateful: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn in_memory() -> Self {
        Self::new(None)
    }

    /// Append one entry to a logged table's log. `rows` is the table's row
    /// count after it (it sets the next checkpoint interval) and
    /// `history_ms` its retention. Returns a checkpoint to take, if one is due: the caller
    /// clones the rows under its lock and writes them after.
    pub fn record_logged_operation(
        &self,
        module: &str,
        table: &str,
        operation: LogOperation,
        stamp: LogStamp,
        rows: usize,
        history_ms: Option<u64>,
    ) -> Result<Option<SnapshotPlan>, IntersticeError> {
        let Some(root) = &self.modules_root else {
            return Ok(None);
        };
        let state = self.get_or_create_state(module, table, PersistenceKind::Logged)?;
        let mut guard = state.lock();
        guard.persistence = PersistenceKind::Logged;
        guard.history_ms = history_ms;
        let seq = guard.next_seq;
        guard.next_seq += 1;

        let start = guard.segment_start;
        let path = match &guard.segment_path {
            Some((s, path)) if *s == start => path.clone(),
            _ => {
                let path = segment_path(root, module, table, start);
                guard.segment_path = Some((start, path.clone()));
                path
            }
        };
        let entry = LogEntry { seq, tx: stamp.0, timestamp_ms: stamp.1, op: operation };
        // Write without fsync — background WAL thread fsyncs every 10ms.
        self.write_log_entry_async(&path, &entry, module, table)?;

        guard.entries_since_checkpoint += 1;
        if guard.entries_since_checkpoint < CHECKPOINT_MIN_INTERVAL.max(guard.rows_at_checkpoint) {
            return Ok(None);
        }
        guard.rows_at_checkpoint = rows as u64;
        // Rotate now, under the table lock: later entries go to the next
        // segment even though the checkpoint is written after the lock.
        guard.entries_since_checkpoint = 0;
        guard.segment_start = seq + 1;
        if let Some(writer) = self.wal_writers.lock().get_mut(&path) {
            writer.retired = true;
        }
        Ok(Some(SnapshotPlan {
            module: module.to_string(),
            table: table.to_string(),
            seq,
            tx: stamp.0,
            timestamp_ms: stamp.1,
        }))
    }

    /// Write a planned checkpoint, then drop history past the retention.
    pub fn snapshot_logged_table(
        &self,
        plan: SnapshotPlan,
        rows: Vec<Row>,
    ) -> Result<(), IntersticeError> {
        let Some(root) = &self.modules_root else {
            return Ok(());
        };
        // Make the log durable through this seq first.
        self.flush_wal();
        let dir = snapshot_dir(root, &plan.module, &plan.table);
        create_dir(&dir)?;
        let checkpoint = Checkpoint {
            version: SNAPSHOT_VERSION,
            seq: plan.seq,
            tx: plan.tx,
            timestamp_ms: plan.timestamp_ms,
            rows,
        };
        write_atomically(&dir.join(checkpoint_name(&checkpoint)), &encode_internal(&checkpoint)?)?;

        let history_ms = self
            .tables
            .lock()
            .get(&TableKey::new(&plan.module, &plan.table))
            .and_then(|state| state.lock().history_ms);
        if let Some(history_ms) = history_ms {
            self.prune(root, &plan.module, &plan.table, now_ms().saturating_sub(history_ms))?;
        }
        Ok(())
    }

    /// Flush all pending WAL writes to disk, and close retired segments.
    ///
    /// Called by the background WAL thread every 10ms.
    /// Also called synchronously before writing a snapshot.
    pub fn flush_wal(&self) {
        // 1. Collect dirty log file paths and mark them clean — hold the lock
        //    only for this brief step, NOT during the actual sync_data call.
        //    This prevents sync_data (which can take ~5ms) from blocking the
        //    reducer loop that also acquires wal_writers to append entries.
        let dirty_paths: Vec<PathBuf> = {
            let mut writers = self.wal_writers.lock();
            // Whatever a commit has not written yet goes first.
            for writer in writers.values_mut() {
                let _ = writer.write_pending();
            }
            let paths: Vec<PathBuf> = writers
                .values()
                .filter(|w| w.dirty)
                .map(|w| w.path.clone())
                .collect();
            for writer in writers.values_mut() {
                writer.dirty = false;
            }
            // A retired segment gets no more entries; it is synced below.
            writers.retain(|_, w| !w.retired);
            paths
        };

        // Sync each dirty file using a fresh read-only handle so the write
        // handle (and its lock) stays free for the reducer loop.
        for path in dirty_paths {
            if let Ok(file) = File::open(&path) {
                let _ = file.sync_data();
            }
        }
    }

    fn stateful_dir(root: &Path, module: &str, table: &str) -> PathBuf {
        root.join(module).join("stateful").join(table)
    }

    fn pk_filename(pk: &IndexKey) -> String {
        // Hex-encode the msgpack-serialized primary key → safe filename
        match encode(pk) {
            Ok(bytes) => bytes.iter().map(|b| format!("{:02x}", b)).collect(),
            Err(_) => "unknown".to_string(),
        }
    }

    /// Stage a stateful row insert/update in the dirty map.
    /// The background flush thread writes it to disk within 10ms.
    pub fn persist_stateful_insert(
        &self,
        module: &str,
        table: &str,
        pk: &IndexKey,
        row: &Row,
    ) -> Result<(), IntersticeError> {
        if self.modules_root.is_none() {
            return Ok(());
        }
        let key = StatefulRowKey {
            module: module.to_string(),
            table: table.to_string(),
            pk: pk.clone(),
        };
        self.dirty_stateful.lock().insert(key, Some(row.clone()));
        Ok(())
    }

    pub fn persist_stateful_update(
        &self,
        module: &str,
        table: &str,
        pk: &IndexKey,
        row: &Row,
    ) -> Result<(), IntersticeError> {
        self.persist_stateful_insert(module, table, pk, row)
    }

    /// Stage a stateful row deletion in the dirty map (None = delete on flush).
    pub fn persist_stateful_delete(
        &self,
        module: &str,
        table: &str,
        pk: &IndexKey,
    ) -> Result<(), IntersticeError> {
        if self.modules_root.is_none() {
            return Ok(());
        }
        let key = StatefulRowKey {
            module: module.to_string(),
            table: table.to_string(),
            pk: pk.clone(),
        };
        self.dirty_stateful.lock().insert(key, None);
        Ok(())
    }

    /// Clear: flush immediately (table reset is rare and correctness-critical).
    pub fn persist_stateful_clear(
        &self,
        module: &str,
        table: &str,
    ) -> Result<(), IntersticeError> {
        let Some(root) = &self.modules_root else {
            return Ok(());
        };
        // Drop all pending dirty writes for this table — they're about to be irrelevant.
        self.dirty_stateful.lock().retain(|k, _| !(k.module == module && k.table == table));
        // Synchronously wipe the directory (clear is rare, correctness beats latency).
        let dir = Self::stateful_dir(root, module, table);
        if dir.exists() {
            fs::remove_dir_all(&dir).map_err(|e| {
                IntersticeError::Internal(format!("Failed to clear stateful dir: {}", e))
            })?;
            fs::create_dir_all(&dir).map_err(|e| {
                IntersticeError::Internal(format!("Failed to recreate stateful dir: {}", e))
            })?;
        }
        Ok(())
    }

    /// Flush all pending stateful row writes to disk.
    /// Called by the background WAL thread every 10ms alongside `flush_wal`.
    pub fn flush_stateful(&self) {
        let Some(root) = &self.modules_root else {
            return;
        };
        // Swap with an empty map — hold the lock only for this instant.
        let batch = {
            let mut guard = self.dirty_stateful.lock();
            if guard.is_empty() {
                return;
            }
            std::mem::take(&mut *guard)
        };

        // Each row file is synced before it replaces the old one, and each
        // directory once after its renames and removals (DESIGN §3.2).
        let mut dirs = std::collections::HashSet::new();
        for (key, row_opt) in batch {
            let dir = Self::stateful_dir(root, &key.module, &key.table);
            let _ = fs::create_dir_all(&dir);
            let path = dir.join(format!("{}.row", Self::pk_filename(&key.pk)));
            match row_opt {
                Some(row) => {
                    if let Ok(encoded) = encode(&row) {
                        let tmp = path.with_extension("row.tmp");
                        if let Ok(mut f) = File::create(&tmp) {
                            if f.write_all(&encoded).and_then(|_| f.sync_data()).is_ok() {
                                let _ = fs::rename(&tmp, &path);
                            }
                        }
                    }
                }
                None => {
                    let _ = fs::remove_file(&path);
                }
            }
            dirs.insert(dir);
        }
        for dir in dirs {
            if let Ok(dir) = File::open(&dir) {
                let _ = dir.sync_all();
            }
        }
    }

    /// Repopulate `table` from disk. Returns the largest transaction id and
    /// timestamp found in its log, so the runtime can continue above them.
    pub fn restore_table(
        &self,
        module: &str,
        table: &mut Table,
    ) -> Result<LogStamp, IntersticeError> {
        let Some(root) = &self.modules_root else {
            return Ok((0, 0));
        };

        if table.schema.persistence == PersistenceKind::Ephemeral {
            let state =
                self.get_or_create_state(module, &table.schema.name, PersistenceKind::Ephemeral)?;
            let mut guard = state.lock();
            *guard = TableState::new(PersistenceKind::Ephemeral);
            return Ok((0, 0));
        }

        // Stateful: restore from per-row files (no WAL)
        if table.schema.persistence == PersistenceKind::Stateful {
            replay_table(root, module, table)?;
            let state = self.get_or_create_state(module, &table.schema.name, PersistenceKind::Stateful)?;
            let mut guard = state.lock();
            guard.persistence = PersistenceKind::Stateful;
            return Ok((0, 0));
        }

        // Logged: the legacy single-file layout is migrated first.
        let table_name = table.schema.name.clone();
        let (max_stamp, next_seq, entries_since_checkpoint) =
            match self.restore_legacy(root, module, table)? {
                Some(next_seq) => ((0, 0), next_seq, 0),
                None => replay_table(root, module, table)?,
            };

        let state = self.get_or_create_state(module, &table_name, PersistenceKind::Logged)?;
        let mut guard = state.lock();
        guard.persistence = PersistenceKind::Logged;
        guard.history_ms = table.schema.history;
        guard.next_seq = next_seq;
        // Never append to an old segment: it may end in a torn entry.
        guard.segment_start = next_seq;
        guard.entries_since_checkpoint = entries_since_checkpoint;
        guard.rows_at_checkpoint = table.scan().len() as u64;
        Ok(max_stamp)
    }

    /// Restore from the pre-history layout (`logs/<t>.log`,
    /// `snapshots/<t>.snap`), write the state as the first checkpoint of the
    /// new layout and remove the legacy files. Returns the next seq, or
    /// `None` when there is nothing legacy.
    fn restore_legacy(
        &self,
        root: &Path,
        module: &str,
        table: &mut Table,
    ) -> Result<Option<u64>, IntersticeError> {
        let name = table.schema.name.clone();
        let log_path = root.join(module).join("logs").join(format!("{name}.log"));
        let snapshot_path = root.join(module).join("snapshots").join(format!("{name}.snap"));
        if !log_path.is_file() && !snapshot_path.is_file() {
            return Ok(None);
        }
        let mut last_seq = None;
        if snapshot_path.is_file() {
            let snapshot: LegacySnapshot = decode_internal(&read_file(&snapshot_path)?)?;
            table.restore_from_rows(snapshot.rows)?;
            last_seq = Some(snapshot.last_seq);
        }
        if log_path.is_file() {
            read_entries::<LegacyLogEntry>(&log_path, |entry| {
                if last_seq.is_none_or(|last| entry.seq > last) {
                    apply_entry(table, &entry.operation)?;
                    last_seq = Some(entry.seq);
                }
                Ok(true)
            })?;
        }
        let next_seq = match last_seq {
            Some(seq) => {
                let dir = snapshot_dir(root, module, &name);
                create_dir(&dir)?;
                // Transaction 0: every point covers it, so it is the start.
                let checkpoint = Checkpoint {
                    version: SNAPSHOT_VERSION,
                    seq,
                    tx: 0,
                    timestamp_ms: 0,
                    rows: table.snapshot_rows(),
                };
                write_atomically(&dir.join(checkpoint_name(&checkpoint)), &encode_internal(&checkpoint)?)?;
                seq + 1
            }
            None => 0,
        };
        let _ = fs::remove_file(&log_path);
        let _ = fs::remove_file(&snapshot_path);
        // `0` means "nothing restored": no entry and no snapshot.
        Ok(Some(next_seq).filter(|n| *n > 0))
    }

    // ── History reads (DESIGN §6.7) ─────────────────────────────────────────

    /// The newest `limit` commits of a logged table, newest first.
    pub fn history_commits(
        &self,
        module: &str,
        table: &str,
        limit: usize,
    ) -> Result<Vec<HistoryCommit>, String> {
        let root = self.history_root()?;
        let files = HistoryFiles::list(root, module, table).map_err(|e| e.to_string())?;
        let mut commits: VecDeque<HistoryCommit> = VecDeque::new();
        files
            .for_each_entry(files.base_seq(), |entry| {
                let same = commits.back().is_some_and(|c| c.tx == entry.tx);
                if !same {
                    if commits.len() == limit {
                        commits.pop_front();
                    }
                    commits.push_back(HistoryCommit {
                        tx: entry.tx,
                        timestamp_ms: entry.timestamp_ms,
                        ops: 0,
                        restored_to: None,
                    });
                }
                if let Some(commit) = commits.back_mut() {
                    match entry.op {
                        LogOperation::Restore { to } => commit.restored_to = Some(to),
                        _ => commit.ops += 1,
                    }
                }
                Ok(true)
            })
            .map_err(|e| e.to_string())?;
        if limit == 0 {
            return Ok(Vec::new());
        }
        Ok(commits.into_iter().rev().collect())
    }

    /// The rows of a logged table at `point`, by primary key, and the `tx`
    /// of the last entry the point covers (`0` for an empty start).
    pub fn history_rows_at(
        &self,
        module: &str,
        schema: &TableSchema,
        point: HistoryPoint,
    ) -> Result<(Vec<Row>, u64), String> {
        let root = self.history_root()?;
        let files = HistoryFiles::list(root, module, &schema.name).map_err(|e| e.to_string())?;
        let cover = match point {
            HistoryPoint::Tx(id) => Cover::Tx(id),
            HistoryPoint::Time(ms) => Cover::Time(ms),
            HistoryPoint::Start => Cover::Tx(files.base_stamp().0),
            HistoryPoint::Back(n) => {
                let commits = self.history_commits(module, &schema.name, n as usize + 1)?;
                match commits.get(n as usize) {
                    Some(commit) => Cover::Tx(commit.tx),
                    None => Cover::Tx(files.base_stamp().0),
                }
            }
        };
        if !cover.covers(files.base_stamp()) {
            return Err(format!("{point:?} is older than the retained history of '{}'", schema.name));
        }

        let mut table = Table::new(schema.clone());
        let mut last_tx = 0;
        let mut after = None;
        if let Some(cp) = files.checkpoints.iter().rev().find(|cp| cover.covers((cp.tx, cp.timestamp_ms))) {
            let checkpoint = read_checkpoint(&cp.path).map_err(|e| e.to_string())?;
            table.restore_from_rows(checkpoint.rows).map_err(|e| e.to_string())?;
            last_tx = cp.tx;
            after = Some(cp.seq);
        }
        files
            .for_each_entry(after, |entry| {
                if !cover.covers((entry.tx, entry.timestamp_ms)) {
                    return Ok(false);
                }
                apply_entry(&mut table, &entry.op)?;
                last_tx = entry.tx;
                Ok(true)
            })
            .map_err(|e| e.to_string())?;

        let mut rows: Vec<(IndexKey, Row)> = table
            .snapshot_rows()
            .into_iter()
            .filter_map(|row| IndexKey::try_from(&row.primary_key).ok().map(|pk| (pk, row)))
            .collect();
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        Ok((rows.into_iter().map(|(_, row)| row).collect(), last_tx))
    }

    fn history_root(&self) -> Result<&Path, String> {
        self.modules_root
            .as_deref()
            .ok_or_else(|| "this node has no data directory, so tables keep no history".to_string())
    }

    /// Drop checkpoints and segments older than the newest checkpoint at or
    /// before `horizon_ms`, which becomes the start of the history.
    fn prune(&self, root: &Path, module: &str, table: &str, horizon_ms: u64) -> Result<(), IntersticeError> {
        let files = HistoryFiles::list(root, module, table)?;
        let Some(base) = files.checkpoints.iter().rev().find(|cp| cp.timestamp_ms <= horizon_ms) else {
            return Ok(());
        };
        for cp in files.checkpoints.iter().filter(|cp| cp.seq < base.seq) {
            let _ = fs::remove_file(&cp.path);
        }
        // Every checkpoint ends a segment (the writer rotates when it is
        // planned), so the segments starting at or before the base hold only
        // entries at or before it.
        for (first, path) in &files.segments {
            if *first <= base.seq {
                let _ = fs::remove_file(path);
            }
        }
        Ok(())
    }

    pub fn clear_all(&self) -> Result<(), IntersticeError> {
        let Some(root) = &self.modules_root else {
            return Ok(());
        };

        // Close all open WAL file handles before clearing.
        self.wal_writers.lock().clear();

        if root.exists() {
            for entry in fs::read_dir(root).map_err(|err| {
                IntersticeError::Internal(format!("Unable to read modules dir: {err}"))
            })? {
                if let Ok(entry) = entry {
                    let path = entry.path();
                    if path.is_dir() {
                        let logs = path.join("logs");
                        if logs.exists() {
                            fs::remove_dir_all(&logs).map_err(|err| {
                                IntersticeError::Internal(format!(
                                    "Failed to clear logs for {:?}: {}",
                                    logs, err
                                ))
                            })?;
                        }
                        fs::create_dir_all(&logs).map_err(|err| {
                            IntersticeError::Internal(format!(
                                "Failed to recreate logs dir {:?}: {}",
                                logs, err
                            ))
                        })?;

                        let snapshots = path.join("snapshots");
                        if snapshots.exists() {
                            fs::remove_dir_all(&snapshots).map_err(|err| {
                                IntersticeError::Internal(format!(
                                    "Failed to clear snapshots for {:?}: {}",
                                    snapshots, err
                                ))
                            })?;
                        }
                        fs::create_dir_all(&snapshots).map_err(|err| {
                            IntersticeError::Internal(format!(
                                "Failed to recreate snapshots dir {:?}: {}",
                                snapshots, err
                            ))
                        })?;

                        let stateful = path.join("stateful");
                        if stateful.exists() {
                            fs::remove_dir_all(&stateful).map_err(|err| {
                                IntersticeError::Internal(format!(
                                    "Failed to clear stateful for {:?}: {}",
                                    stateful, err
                                ))
                            })?;
                        }
                        // Don't recreate stateful dir — it's created on demand per table
                    }
                }
            }
        }

        self.tables.lock().clear();
        Ok(())
    }

    pub fn cleanup_module(&self, module: &str) {
        self.tables.lock().retain(|key, _| key.module != module);
        // Close WAL writers for this module's log files.
        if let Some(root) = &self.modules_root {
            let module_log_dir = root.join(module).join("logs");
            self.wal_writers
                .lock()
                
                .retain(|path, _| !path.starts_with(&module_log_dir));
        }
        // Remove stateful row files for this module.
        if let Some(root) = &self.modules_root {
            let stateful_module_dir = root.join(module).join("stateful");
            if stateful_module_dir.exists() {
                let _ = fs::remove_dir_all(&stateful_module_dir);
            }
        }
    }

    fn get_or_create_state(
        &self,
        module: &str,
        table: &str,
        persistence: PersistenceKind,
    ) -> Result<Arc<Mutex<TableState>>, IntersticeError> {
        let mut tables = self.tables.lock();
        if let Some(state) = tables.get(&TableKey::new(module, table)) {
            return Ok(state.clone());
        }

        let state = Arc::new(Mutex::new(TableState::new(persistence)));
        tables.insert(TableKey::new(module, table), state.clone());
        Ok(state)
    }

    /// Write a log entry to disk without fsync (async WAL hot path).
    /// Reuses an open file handle; opens one on first write to this path.
    fn write_log_entry_async(
        &self,
        path: &PathBuf,
        entry: &LogEntry,
        module: &str,
        table: &str,
    ) -> Result<(), IntersticeError> {
        let encoded = encode_internal(entry)?;
        let length = (encoded.len() as u32).to_le_bytes();
        let mut writers = self.wal_writers.lock();
        if !writers.contains_key(path) {
            if let Some(root) = &self.modules_root {
                create_dir(&root.join(module).join("logs").join(table))?;
            }
            let file = OpenOptions::new().create(true).append(true).open(path).map_err(|err| {
                IntersticeError::Internal(format!("Failed to open log file {:?}: {}", path, err))
            })?;
            writers.insert(
                path.clone(),
                WalWriter { file, pending: Vec::new(), path: path.clone(), dirty: false, retired: false },
            );
        }
        let writer = writers.get_mut(path).expect("inserted above");
        writer.pending.extend_from_slice(&length);
        writer.pending.extend_from_slice(&encoded);
        Ok(())
    }

    /// Hand the log entries buffered by the current commit to the OS: one
    /// write per log file. Called at the end of every commit, so a committed
    /// transaction is in the kernel as before; the flush thread syncs it.
    pub fn write_pending(&self) -> Result<(), IntersticeError> {
        let mut writers = self.wal_writers.lock();
        for writer in writers.values_mut() {
            writer.write_pending()?;
        }
        Ok(())
    }

    pub fn forget_module(&self, module: &str) {
        self.cleanup_module(module);
    }

    // ── Migrations (DESIGN §3.3) ────────────────────────────────────────────

    /// The table's current rows, read from disk in the layout of `schema`
    /// without registering any state.
    pub fn read_rows(&self, module: &str, schema: &TableSchema) -> Result<Vec<Row>, IntersticeError> {
        let Some(root) = &self.modules_root else {
            return Ok(Vec::new());
        };
        if schema.persistence == PersistenceKind::Ephemeral {
            return Ok(Vec::new());
        }
        let mut scratch = Table::new(schema.clone());
        replay_table(root, module, &mut scratch)?;
        Ok(scratch.snapshot_rows())
    }

    /// Flush everything pending and forget the module's open files and
    /// table state, keeping the data on disk.
    pub fn release_module(&self, module: &str) {
        self.flush_stateful();
        self.flush_wal();
        self.tables.lock().retain(|key, _| key.module != module);
        if let Some(root) = &self.modules_root {
            let module_log_dir = root.join(module).join("logs");
            self.wal_writers.lock().retain(|path, _| !path.starts_with(&module_log_dir));
        }
    }

    /// Delete all of a module's table data (`MigrationPolicy::DeleteData`).
    pub fn delete_module_data(&self, module: &str) -> Result<(), IntersticeError> {
        self.release_module(module);
        let Some(root) = &self.modules_root else { return Ok(()) };
        for kind in DATA_KINDS {
            remove_path(&root.join(module).join(kind))?;
        }
        Ok(())
    }

    /// Rewrite the module's data with `actions`, crash-safely: stage into
    /// `.migrate/`, mark it `READY`, swap each touched table in, then move
    /// the new `schema.bin`. Nothing changes if staging fails.
    pub fn migrate_module(
        &self,
        module: &str,
        new: &interstice_abi::ModuleSchema,
        old_tables: &[TableSchema],
        actions: &[crate::runtime::migration::TableAction],
        schema_bytes: &[u8],
    ) -> Result<(), IntersticeError> {
        use crate::runtime::migration::{TableAction, map_row};
        let Some(root) = &self.modules_root else { return Ok(()) };
        self.release_module(module);
        let dir = root.join(module);
        let stage = dir.join(MIGRATE_DIR);
        remove_path(&stage)?;
        create_dir(&stage)?;
        let staged = (|| -> Result<Vec<String>, IntersticeError> {
            let mut ready = Vec::new();
            for action in actions {
                match action {
                    TableAction::Remove { table } => ready.push(format!("remove {table}")),
                    TableAction::Map { table, columns } => {
                        let old = old_tables.iter().find(|t| &t.name == table).ok_or_else(|| {
                            IntersticeError::Internal(format!("migration: no old schema for table {table}"))
                        })?;
                        let new_table = new.tables.iter().find(|t| &t.name == table).ok_or_else(|| {
                            IntersticeError::Internal(format!("migration: no new schema for table {table}"))
                        })?;
                        let map = |row: Row| -> Result<Row, IntersticeError> {
                            let row = map_row(row, columns);
                            new_table.validate_row(&row, &new.type_definitions).map_err(|e| {
                                IntersticeError::Internal(format!("migration: a row of {table} doesn't fit: {e}"))
                            })?;
                            Ok(row)
                        };
                        stage_mapped_table(&dir, &stage, table, &old.persistence, map)?;
                        ready.push(format!("map {table}"));
                    }
                }
            }
            Ok(ready)
        })();
        let ready = match staged {
            Ok(ready) => ready,
            Err(e) => {
                let _ = remove_path(&stage);
                return Err(e);
            }
        };
        write_atomically(&stage.join("schema.bin"), schema_bytes)?;
        write_atomically(&stage.join("READY"), ready.join("\n").as_bytes())?;
        finish_migration(&dir)
    }

    /// Finish or discard a migration interrupted by a crash.
    pub fn recover_migration(&self, module: &str) -> Result<(), IntersticeError> {
        let Some(root) = &self.modules_root else { return Ok(()) };
        let dir = root.join(module);
        let stage = dir.join(MIGRATE_DIR);
        if !stage.exists() {
            return Ok(());
        }
        if stage.join("READY").exists() {
            finish_migration(&dir)
        } else {
            remove_path(&stage)
        }
    }
}

const DATA_KINDS: [&str; 3] = ["stateful", "logs", "snapshots"];
const MIGRATE_DIR: &str = ".migrate";

/// Swap the staged tables listed in `READY` in. Idempotent, so a crash
/// part-way is finished by running it again.
fn finish_migration(dir: &Path) -> Result<(), IntersticeError> {
    let stage = dir.join(MIGRATE_DIR);
    let ready = String::from_utf8_lossy(&read_file(&stage.join("READY"))?).to_string();
    for line in ready.lines() {
        let Some((op, table)) = line.split_once(' ') else { continue };
        for kind in DATA_KINDS {
            let live = dir.join(kind).join(table);
            let staged = stage.join(kind).join(table);
            match op {
                "remove" => remove_path(&live)?,
                // Once the staged copy is gone it has been moved in.
                "map" if staged.exists() => {
                    remove_path(&live)?;
                    create_dir(&dir.join(kind))?;
                    fs::rename(&staged, &live).map_err(|e| {
                        IntersticeError::Internal(format!("migration: failed to move {staged:?}: {e}"))
                    })?;
                }
                _ => {}
            }
        }
    }
    let schema = stage.join("schema.bin");
    if schema.exists() {
        fs::rename(&schema, dir.join("schema.bin")).map_err(|e| {
            IntersticeError::Internal(format!("migration: failed to move schema.bin: {e}"))
        })?;
    }
    remove_path(&stage)
}

/// Write `table`'s data, every row passed through `map`, under `stage`.
/// Logged tables keep their whole history: every log entry and checkpoint.
fn stage_mapped_table(
    dir: &Path,
    stage: &Path,
    table: &str,
    persistence: &PersistenceKind,
    map: impl Fn(Row) -> Result<Row, IntersticeError>,
) -> Result<(), IntersticeError> {
    match persistence {
        PersistenceKind::Ephemeral => Ok(()),
        PersistenceKind::Stateful => {
            let out = stage.join("stateful").join(table);
            create_dir(&out)?;
            for path in list_dir(&dir.join("stateful").join(table))? {
                if path.extension().and_then(|e| e.to_str()) != Some("row") {
                    continue;
                }
                let row: Row = decode_internal(&read_file(&path)?)?;
                let name = path.file_name().expect("listed file");
                write_atomically(&out.join(name), &encode_internal(&map(row)?)?)?;
            }
            Ok(())
        }
        PersistenceKind::Logged => {
            let out = stage.join("logs").join(table);
            create_dir(&out)?;
            for path in list_dir(&dir.join("logs").join(table))? {
                if path.extension().and_then(|e| e.to_str()) != Some("log") {
                    continue;
                }
                let mut bytes = Vec::new();
                read_entries::<LogEntry>(&path, |mut entry| {
                    entry.op = match entry.op {
                        LogOperation::Insert { primary_key, row } => {
                            LogOperation::Insert { primary_key, row: row.map(&map).transpose()? }
                        }
                        LogOperation::Update { primary_key, row } => {
                            LogOperation::Update { primary_key, row: row.map(&map).transpose()? }
                        }
                        op => op,
                    };
                    let encoded = encode_internal(&entry)?;
                    bytes.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
                    bytes.extend_from_slice(&encoded);
                    Ok(true)
                })?;
                write_atomically(&out.join(path.file_name().expect("listed file")), &bytes)?;
            }
            let out = stage.join("snapshots").join(table);
            create_dir(&out)?;
            for path in list_dir(&dir.join("snapshots").join(table))? {
                if path.extension().and_then(|e| e.to_str()) != Some("snap") {
                    continue;
                }
                let mut cp = read_checkpoint(&path)?;
                cp.rows = cp.rows.into_iter().map(&map).collect::<Result<_, _>>()?;
                write_atomically(&out.join(path.file_name().expect("listed file")), &encode_internal(&cp)?)?;
            }
            Ok(())
        }
    }
}

fn remove_path(path: &Path) -> Result<(), IntersticeError> {
    let result = if path.is_dir() {
        fs::remove_dir_all(path)
    } else if path.exists() {
        fs::remove_file(path)
    } else {
        return Ok(());
    };
    result.map_err(|e| IntersticeError::Internal(format!("Failed to remove {path:?}: {e}")))
}

/// Replay a table's persisted data into `table`: stateful row files, or
/// the newest checkpoint plus the log after it. Returns the largest stamp,
/// the next seq and the entries since the checkpoint.
fn replay_table(root: &Path, module: &str, table: &mut Table) -> Result<(LogStamp, u64, u64), IntersticeError> {
    if table.schema.persistence == PersistenceKind::Stateful {
        let dir = TableStore::stateful_dir(root, module, &table.schema.name);
        for path in list_dir(&dir)? {
            if path.extension().and_then(|e| e.to_str()) == Some("row") {
                let row: Row = decode_internal(&read_file(&path)?)?;
                table.insert(row)?;
            }
        }
        return Ok(((0, 0), 0, 0));
    }
    let files = HistoryFiles::list(root, module, &table.schema.name)?;
    let mut max_stamp: LogStamp = (0, 0);
    let mut next_seq = 0;
    let mut entries_since_checkpoint = 0;
    let mut after = None;
    if let Some(cp) = files.checkpoints.last() {
        let checkpoint = read_checkpoint(&cp.path)?;
        table.restore_from_rows(checkpoint.rows)?;
        max_stamp = (cp.tx, cp.timestamp_ms);
        next_seq = cp.seq + 1;
        after = Some(cp.seq);
    }
    files.for_each_entry(after, |entry| {
        apply_entry(table, &entry.op)?;
        max_stamp = (max_stamp.0.max(entry.tx), max_stamp.1.max(entry.timestamp_ms));
        next_seq = entry.seq + 1;
        entries_since_checkpoint += 1;
        Ok(true)
    })?;
    Ok((max_stamp, next_seq, entries_since_checkpoint))
}

// ── On-disk format ───────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct LogEntry {
    seq: u64,
    tx: u64,
    timestamp_ms: u64,
    op: LogOperation,
}

#[derive(Serialize, Deserialize)]
struct Checkpoint {
    version: u16,
    /// The state after entry `seq`.
    seq: u64,
    /// The transaction and timestamp of entry `seq`.
    tx: u64,
    timestamp_ms: u64,
    rows: Vec<Row>,
}

/// Pre-history formats, read once to migrate.
#[derive(Serialize, Deserialize)]
struct LegacyLogEntry {
    seq: u64,
    timestamp_ms: u64,
    operation: LogOperation,
}

#[derive(Serialize, Deserialize)]
struct LegacySnapshot {
    version: u16,
    last_seq: u64,
    rows: Vec<Row>,
}

fn segment_path(root: &Path, module: &str, table: &str, first_seq: u64) -> PathBuf {
    root.join(module).join("logs").join(table).join(format!("{first_seq:020}.log"))
}

fn snapshot_dir(root: &Path, module: &str, table: &str) -> PathBuf {
    root.join(module).join("snapshots").join(table)
}

/// `<seq>-<tx>-<ts>.snap`: the header is in the name, so choosing a
/// checkpoint doesn't read any.
fn checkpoint_name(cp: &Checkpoint) -> String {
    format!("{:020}-{}-{}.snap", cp.seq, cp.tx, cp.timestamp_ms)
}

struct CheckpointFile {
    seq: u64,
    tx: u64,
    timestamp_ms: u64,
    path: PathBuf,
}

/// The segments and checkpoints of one table, sorted by seq.
struct HistoryFiles {
    segments: Vec<(u64, PathBuf)>,
    checkpoints: Vec<CheckpointFile>,
}

impl HistoryFiles {
    fn list(root: &Path, module: &str, table: &str) -> Result<Self, IntersticeError> {
        let mut segments = Vec::new();
        for path in list_dir(&root.join(module).join("logs").join(table))? {
            let first = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".log"))
                .and_then(|n| n.parse::<u64>().ok());
            if let Some(first) = first {
                segments.push((first, path));
            }
        }
        segments.sort_by_key(|(first, _)| *first);
        let mut checkpoints = Vec::new();
        for path in list_dir(&snapshot_dir(root, module, table))? {
            let header = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".snap"))
                .map(|n| n.split('-').filter_map(|p| p.parse::<u64>().ok()).collect::<Vec<_>>());
            if let Some([seq, tx, timestamp_ms]) = header.as_deref() {
                checkpoints.push(CheckpointFile { seq: *seq, tx: *tx, timestamp_ms: *timestamp_ms, path });
            }
        }
        checkpoints.sort_by_key(|cp| cp.seq);
        Ok(Self { segments, checkpoints })
    }

    /// Whether the history starts from the empty table: nothing was pruned
    /// or migrated, so the first segment starts at seq 0.
    fn starts_empty(&self) -> bool {
        match self.segments.first() {
            Some((first, _)) => *first == 0,
            None => self.checkpoints.is_empty(),
        }
    }

    /// The seq the history starts after (`None`: the empty table).
    fn base_seq(&self) -> Option<u64> {
        if self.starts_empty() { None } else { self.checkpoints.first().map(|cp| cp.seq) }
    }

    /// The stamp of the oldest retained state.
    fn base_stamp(&self) -> LogStamp {
        match self.checkpoints.first() {
            Some(cp) if !self.starts_empty() => (cp.tx, cp.timestamp_ms),
            _ => (0, 0),
        }
    }

    /// Visit entries with seq greater than `after`, in order, until the
    /// visitor returns `false`.
    fn for_each_entry(
        &self,
        after: Option<u64>,
        mut visit: impl FnMut(LogEntry) -> Result<bool, IntersticeError>,
    ) -> Result<(), IntersticeError> {
        let min_seq = after.map_or(0, |s| s + 1);
        for (i, (_, path)) in self.segments.iter().enumerate() {
            // Skip segments that end before `min_seq`.
            if self.segments.get(i + 1).is_some_and(|(next, _)| *next <= min_seq) {
                continue;
            }
            let mut go_on = true;
            read_entries::<LogEntry>(path, |entry| {
                if entry.seq < min_seq {
                    return Ok(true);
                }
                go_on = visit(entry)?;
                Ok(go_on)
            })?;
            if !go_on {
                break;
            }
        }
        Ok(())
    }
}

/// Which entries a point covers.
#[derive(Clone, Copy)]
enum Cover {
    Tx(u64),
    Time(u64),
}

impl Cover {
    fn covers(self, (tx, timestamp_ms): LogStamp) -> bool {
        match self {
            Cover::Tx(id) => tx <= id,
            Cover::Time(ms) => timestamp_ms <= ms,
        }
    }
}

fn apply_entry(table: &mut Table, op: &LogOperation) -> Result<(), IntersticeError> {
    match op {
        LogOperation::Insert { row, .. } => {
            let row = row.clone().ok_or_else(|| {
                IntersticeError::Internal("Missing row data for log insert".into())
            })?;
            table.insert(row)?;
        }
        LogOperation::Update { row, .. } => {
            let row = row.clone().ok_or_else(|| {
                IntersticeError::Internal("Missing row data for log update".into())
            })?;
            table.update(row)?;
        }
        LogOperation::Delete { primary_key } => {
            let _ = table.delete(primary_key)?;
        }
        LogOperation::Clear => table.clear(),
        LogOperation::Restore { .. } => {}
    }
    Ok(())
}

/// Read length-prefixed entries until the end, or until `visit` returns
/// `false`. A torn last entry (a crash mid-write) ends the file.
fn read_entries<E: for<'de> Deserialize<'de>>(
    path: &Path,
    mut visit: impl FnMut(E) -> Result<bool, IntersticeError>,
) -> Result<(), IntersticeError> {
    let Ok(mut file) = File::open(path) else {
        return Ok(());
    };
    loop {
        let mut len_buf = [0u8; 4];
        if file.read_exact(&mut len_buf).is_err() {
            return Ok(());
        }
        let mut buffer = vec![0u8; u32::from_le_bytes(len_buf) as usize];
        if file.read_exact(&mut buffer).is_err() {
            return Ok(());
        }
        let entry: E = decode_internal(&buffer)?;
        if !visit(entry)? {
            return Ok(());
        }
    }
}

fn read_checkpoint(path: &Path) -> Result<Checkpoint, IntersticeError> {
    decode_internal(&read_file(path)?)
}

fn list_dir(dir: &Path) -> Result<Vec<PathBuf>, IntersticeError> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let entries = fs::read_dir(dir)
        .map_err(|e| IntersticeError::Internal(format!("Failed to read {dir:?}: {e}")))?;
    Ok(entries.filter_map(|e| e.ok().map(|e| e.path())).collect())
}

fn create_dir(dir: &Path) -> Result<(), IntersticeError> {
    fs::create_dir_all(dir)
        .map_err(|e| IntersticeError::Internal(format!("Failed to create {dir:?}: {e}")))
}

fn read_file(path: &Path) -> Result<Vec<u8>, IntersticeError> {
    fs::read(path).map_err(|e| IntersticeError::Internal(format!("Failed to read {path:?}: {e}")))
}

fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), IntersticeError> {
    let tmp = path.with_extension("tmp");
    let fail = |e: std::io::Error| IntersticeError::Internal(format!("Failed to write {path:?}: {e}"));
    let mut file = File::create(&tmp).map_err(fail)?;
    file.write_all(bytes).map_err(fail)?;
    file.sync_all().map_err(fail)?;
    fs::rename(&tmp, path).map_err(fail)
}

fn encode_internal<T: Serialize>(value: &T) -> Result<Vec<u8>, IntersticeError> {
    encode(value).map_err(|e| IntersticeError::Internal(format!("Failed to encode: {e}")))
}

fn decode_internal<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T, IntersticeError> {
    decode(bytes).map_err(|e| IntersticeError::Internal(format!("Failed to decode: {e}")))
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use interstice_abi::{FieldDef, IntersticeType, IntersticeValue, TableVisibility};

    fn schema(history: Option<u64>) -> TableSchema {
        TableSchema {
            name: "t".into(),
            type_name: "T".into(),
            visibility: TableVisibility::Private,
            fields: vec![FieldDef { name: "v".into(), field_type: IntersticeType::String }],
            primary_key: FieldDef { name: "id".into(), field_type: IntersticeType::U64 },
            primary_key_auto_inc: false,
            indexes: Vec::new(),
            persistence: PersistenceKind::Logged,
            retention: None,
            history,
            defaults: Vec::new(),
        }
    }

    fn row(id: u64, v: &str) -> Row {
        Row { primary_key: IntersticeValue::U64(id), entries: vec![IntersticeValue::String(v.into())] }
    }

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("interstice-history-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A live table and its store, written the way commits write them.
    struct Fixture {
        store: TableStore,
        table: Table,
    }

    impl Fixture {
        fn new(root: &Path, history: Option<u64>) -> Self {
            Self { store: TableStore::new(Some(root.to_path_buf())), table: Table::new(schema(history)) }
        }

        fn write(&mut self, op: LogOperation, stamp: LogStamp) {
            apply_entry(&mut self.table, &op).unwrap();
            let plan = self
                .store
                .record_logged_operation("m", "t", op, stamp, self.table.scan().len(), self.table.schema.history)
                .unwrap();
            // As at the end of a commit.
            self.store.write_pending().unwrap();
            if let Some(plan) = plan {
                self.store.snapshot_logged_table(plan, self.table.snapshot_rows()).unwrap();
            }
        }

        fn insert(&mut self, id: u64, v: &str, stamp: LogStamp) {
            self.write(LogOperation::Insert { primary_key: IndexKey::U64(id), row: Some(row(id, v)) }, stamp);
        }

        fn rows_at(&self, point: HistoryPoint) -> Result<Vec<(u64, String)>, String> {
            let (rows, _) = self.store.history_rows_at("m", &self.table.schema, point)?;
            Ok(rows
                .into_iter()
                .map(|r| match (&r.primary_key, &r.entries[0]) {
                    (IntersticeValue::U64(id), IntersticeValue::String(v)) => (*id, v.clone()),
                    _ => panic!("bad row"),
                })
                .collect())
        }
    }

    fn rows(pairs: &[(u64, &str)]) -> Vec<(u64, String)> {
        pairs.iter().map(|(id, v)| (*id, v.to_string())).collect()
    }

    #[test]
    fn commits_and_points() {
        let root = temp_root("points");
        let mut f = Fixture::new(&root, None);
        f.insert(1, "a", (1, 100));
        f.insert(2, "b", (2, 200));
        f.write(LogOperation::Update { primary_key: IndexKey::U64(1), row: Some(row(1, "a2")) }, (3, 300));
        f.write(LogOperation::Delete { primary_key: IndexKey::U64(2) }, (3, 300));

        let commits = f.store.history_commits("m", "t", 10).unwrap();
        let summary: Vec<(u64, u32)> = commits.iter().map(|c| (c.tx, c.ops)).collect();
        assert_eq!(summary, vec![(3, 2), (2, 1), (1, 1)]);
        assert_eq!(f.store.history_commits("m", "t", 1).unwrap().len(), 1);

        assert_eq!(f.rows_at(HistoryPoint::Tx(0)).unwrap(), rows(&[]));
        assert_eq!(f.rows_at(HistoryPoint::Tx(1)).unwrap(), rows(&[(1, "a")]));
        assert_eq!(f.rows_at(HistoryPoint::Tx(2)).unwrap(), rows(&[(1, "a"), (2, "b")]));
        assert_eq!(f.rows_at(HistoryPoint::Time(250)).unwrap(), rows(&[(1, "a"), (2, "b")]));
        assert_eq!(f.rows_at(HistoryPoint::Back(0)).unwrap(), rows(&[(1, "a2")]));
        assert_eq!(f.rows_at(HistoryPoint::Back(1)).unwrap(), rows(&[(1, "a"), (2, "b")]));
        assert_eq!(f.rows_at(HistoryPoint::Back(9)).unwrap(), rows(&[]));
        assert_eq!(f.rows_at(HistoryPoint::Start).unwrap(), rows(&[]));

        // A restore is a commit tagged with its target.
        f.write(LogOperation::Restore { to: 1 }, (4, 400));
        f.write(LogOperation::Delete { primary_key: IndexKey::U64(1) }, (4, 400));
        let newest = &f.store.history_commits("m", "t", 1).unwrap()[0];
        assert_eq!((newest.tx, newest.ops, newest.restored_to), (4, 1, Some(1)));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn checkpoints_rotation_and_restart() {
        let root = temp_root("checkpoints");
        let mut f = Fixture::new(&root, None);
        for i in 1..=600u64 {
            f.insert(i, "x", (i, i));
        }
        let files = HistoryFiles::list(&root, "m", "t").unwrap();
        let firsts: Vec<u64> = files.segments.iter().map(|(first, _)| *first).collect();
        assert_eq!(firsts, vec![0, 256, 512]);
        let cps: Vec<(u64, u64)> = files.checkpoints.iter().map(|cp| (cp.seq, cp.tx)).collect();
        assert_eq!(cps, vec![(255, 256), (511, 512)]);

        assert_eq!(f.rows_at(HistoryPoint::Tx(300)).unwrap().len(), 300);
        assert_eq!(f.rows_at(HistoryPoint::Tx(512)).unwrap().len(), 512);
        assert_eq!(f.rows_at(HistoryPoint::Tx(600)).unwrap().len(), 600);
        assert_eq!(f.rows_at(HistoryPoint::Start).unwrap().len(), 0);

        // Restart: same state, ids continue above the log, new segment.
        let store = TableStore::new(Some(root.clone()));
        let mut table = Table::new(schema(None));
        assert_eq!(store.restore_table("m", &mut table).unwrap(), (600, 600));
        assert_eq!(table.scan().len(), 600);
        let mut f = Fixture { store, table };
        f.insert(601, "y", (601, 601));
        let files = HistoryFiles::list(&root, "m", "t").unwrap();
        assert_eq!(files.segments.last().unwrap().0, 600);
        assert_eq!(f.rows_at(HistoryPoint::Back(1)).unwrap().len(), 600);
        assert_eq!(f.rows_at(HistoryPoint::Back(0)).unwrap().len(), 601);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn retention_prunes_to_a_checkpoint() {
        let root = temp_root("prune");
        // Everything is far older than one second, so each checkpoint
        // becomes the new start.
        let mut f = Fixture::new(&root, Some(1000));
        for i in 1..=600u64 {
            f.insert(i, "x", (i, i));
        }
        let files = HistoryFiles::list(&root, "m", "t").unwrap();
        assert_eq!(files.checkpoints.iter().map(|cp| cp.seq).collect::<Vec<_>>(), vec![511]);
        assert_eq!(files.segments.iter().map(|(first, _)| *first).collect::<Vec<_>>(), vec![512]);
        assert!(f.rows_at(HistoryPoint::Tx(100)).is_err());
        assert_eq!(f.rows_at(HistoryPoint::Start).unwrap().len(), 512);
        assert_eq!(f.rows_at(HistoryPoint::Tx(550)).unwrap().len(), 550);
        let commits = f.store.history_commits("m", "t", 1000).unwrap();
        assert_eq!(commits.len(), 600 - 512);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn legacy_layout_is_migrated() {
        let root = temp_root("legacy");
        let logs = root.join("m").join("logs");
        let snaps = root.join("m").join("snapshots");
        fs::create_dir_all(&logs).unwrap();
        fs::create_dir_all(&snaps).unwrap();
        let snapshot = LegacySnapshot { version: 1, last_seq: 1, rows: vec![row(1, "a"), row(2, "b")] };
        fs::write(snaps.join("t.snap"), encode(&snapshot).unwrap()).unwrap();
        let mut log = Vec::new();
        for entry in [
            LegacyLogEntry { seq: 1, timestamp_ms: 5, operation: LogOperation::Clear },
            LegacyLogEntry {
                seq: 2,
                timestamp_ms: 6,
                operation: LogOperation::Insert { primary_key: IndexKey::U64(3), row: Some(row(3, "c")) },
            },
        ] {
            let bytes = encode(&entry).unwrap();
            log.extend((bytes.len() as u32).to_le_bytes());
            log.extend(bytes);
        }
        fs::write(logs.join("t.log"), log).unwrap();

        let store = TableStore::new(Some(root.clone()));
        let mut table = Table::new(schema(None));
        store.restore_table("m", &mut table).unwrap();
        assert_eq!(table.scan().len(), 3);
        assert!(!logs.join("t.log").exists() && !snaps.join("t.snap").exists());

        let mut f = Fixture { store, table };
        assert_eq!(f.rows_at(HistoryPoint::Start).unwrap().len(), 3);
        f.insert(4, "d", (10, 10));
        assert_eq!(f.rows_at(HistoryPoint::Back(1)).unwrap().len(), 3);
        assert_eq!(f.rows_at(HistoryPoint::Back(0)).unwrap().len(), 4);
        let _ = fs::remove_dir_all(&root);
    }

    /// A logged table with checkpoints gets a column appended: every log
    /// entry and checkpoint is rewritten, history still answers, and an
    /// interrupted migration is finished on recovery.
    #[test]
    fn migration_rewrites_log_and_checkpoints() {
        use crate::runtime::migration::{self, TableAction};
        let root = temp_root("migrate");
        let mut f = Fixture::new(&root, None);
        for i in 1..=300u64 {
            f.insert(i, "x", (i, i));
        }
        f.store.flush_wal();
        let old = {
            let mut m = interstice_abi::ModuleSchema::empty();
            m.name = "m".into();
            m.tables = vec![schema(None)];
            m
        };
        let mut new = old.clone();
        new.tables[0].fields.push(FieldDef { name: "n".into(), field_type: IntersticeType::U64 });
        new.tables[0].defaults.push(interstice_abi::ColumnDefault::new("n", &IntersticeValue::U64(7)));
        let store = &f.store;
        let plan = migration::plan(&old, &new, &mut |t| store.read_rows("m", t)).unwrap();
        assert!(!plan.needs_decision(), "{}", plan.render());
        assert!(matches!(plan.actions[..], [TableAction::Map { .. }]));
        f.store.migrate_module("m", &new, &old.tables, &plan.actions, b"schema").unwrap();

        let rows = f.store.read_rows("m", &new.tables[0]).unwrap();
        assert_eq!(rows.len(), 300);
        assert!(rows.iter().all(|r| r.entries[1] == IntersticeValue::U64(7)));
        let (at, _) = f.store.history_rows_at("m", &new.tables[0], HistoryPoint::Tx(100)).unwrap();
        assert_eq!(at.len(), 100);
        assert_eq!(fs::read(root.join("m").join("schema.bin")).unwrap(), b"schema");
        assert!(!root.join("m").join(MIGRATE_DIR).exists());

        // A crash before READY: the staging is discarded.
        fs::create_dir_all(root.join("m").join(MIGRATE_DIR).join("logs").join("t")).unwrap();
        f.store.recover_migration("m").unwrap();
        assert!(!root.join("m").join(MIGRATE_DIR).exists());
        assert_eq!(f.store.read_rows("m", &new.tables[0]).unwrap().len(), 300);

        // Removing the column back needs a decision; applied, rows lose it.
        let plan = migration::plan(&new, &old, &mut |t| store.read_rows("m", t)).unwrap();
        assert!(plan.needs_decision());
        f.store.migrate_module("m", &old, &new.tables, &plan.actions, b"schema2").unwrap();
        let rows = f.store.read_rows("m", &old.tables[0]).unwrap();
        assert!(rows.iter().all(|r| r.entries.len() == 1));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn migration_rewrites_stateful_rows() {
        use crate::runtime::migration::{self, TableAction};
        let root = temp_root("migrate-stateful");
        let store = TableStore::new(Some(root.clone()));
        let mut t = schema(None);
        t.persistence = PersistenceKind::Stateful;
        for i in 1..=3u64 {
            let mut table = Table::new(t.clone());
            let r = row(i, "s");
            table.insert(r.clone()).unwrap();
            store.persist_stateful_insert("m", &t.name, &IndexKey::U64(i), &r).unwrap();
        }
        store.flush_stateful();
        let mut old = interstice_abi::ModuleSchema::empty();
        old.name = "m".into();
        old.tables = vec![t.clone()];
        let mut new = old.clone();
        new.tables[0].fields.push(FieldDef {
            name: "tag".into(),
            field_type: IntersticeType::Option(Box::new(IntersticeType::String)),
        });
        let plan = migration::plan(&old, &new, &mut |t| store.read_rows("m", t)).unwrap();
        assert!(!plan.needs_decision(), "{}", plan.render());
        assert!(matches!(plan.actions[..], [TableAction::Map { .. }]));
        store.migrate_module("m", &new, &old.tables, &plan.actions, b"s").unwrap();
        let rows = store.read_rows("m", &new.tables[0]).unwrap();
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|r| r.entries[1] == IntersticeValue::Option(None)));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn auto_inc_ids_are_never_reused_across_restarts() {
        use crate::runtime::table::auto_inc::SEQUENCE_BLOCK;
        let root = temp_root("sequences");
        let store = Arc::new(TableStore::new(Some(root.clone())));
        let mut schema = schema(None);
        schema.primary_key_auto_inc = true;
        let next = |table: &Table| match table.auto_inc_snapshot().primary.unwrap().next_value().unwrap() {
            IntersticeValue::U64(id) => id,
            other => panic!("{other:?}"),
        };

        let first = Table::new(schema.clone());
        first.persist_sequences(&store, "m", &store.sequence_ceilings("m"));
        let ids: Vec<u64> = (0..10).map(|_| next(&first)).collect();
        assert_eq!(ids, (0..10).collect::<Vec<_>>());
        // The block was saved before the first id was handed out.
        assert_eq!(store.sequence_ceilings("m").get("t.id"), Some(&SEQUENCE_BLOCK));

        // A restart where the rows holding ids 0..10 were all deleted.
        let second = Table::new(schema.clone());
        second.persist_sequences(&store, "m", &store.sequence_ceilings("m"));
        assert!(next(&second) >= 10, "an id was reused");

        // A restored table keeps its counter too.
        let mut third = Table::new(schema);
        third.persist_sequences(&store, "m", &store.sequence_ceilings("m"));
        let before = next(&third);
        third.restore_from_rows(Vec::new()).unwrap();
        assert!(next(&third) > before);
        let _ = fs::remove_dir_all(root);
    }
}
