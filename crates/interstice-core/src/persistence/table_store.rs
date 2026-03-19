use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use interstice_abi::{IndexKey, PersistenceKind, Row, decode, encode};
use serde::{Deserialize, Serialize};

use crate::{error::IntersticeError, runtime::table::Table};

const SNAPSHOT_VERSION: u16 = 1;
const SNAPSHOT_INTERVAL: u64 = 256;

#[derive(Clone, Debug)]
pub struct SnapshotPlan {
    pub module: String,
    pub table: String,
    pub seq: u64,
}

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
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct TableKey {
    module: String,
    table: String,
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
    last_snapshot_seq: u64,
}

impl TableState {
    fn new(persistence: PersistenceKind) -> Self {
        Self {
            persistence,
            next_seq: 0,
            last_snapshot_seq: 0,
        }
    }
}

/// Persistent open file handle for async WAL writes.
struct WalWriter {
    file: File,
    /// Absolute path to this log file (for re-opening a sync fd).
    path: PathBuf,
    /// True when entries have been written but not yet fsynced.
    dirty: bool,
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
}

impl TableStore {
    pub fn new(root: Option<PathBuf>) -> Self {
        Self {
            modules_root: root,
            tables: Mutex::new(HashMap::new()),
            wal_writers: Arc::new(Mutex::new(HashMap::new())),
            dirty_stateful: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn in_memory() -> Self {
        Self::new(None)
    }

    pub fn record_logged_operation(
        &self,
        module: &str,
        table: &str,
        operation: LogOperation,
    ) -> Result<Option<SnapshotPlan>, IntersticeError> {
        let Some(root) = &self.modules_root else {
            return Ok(None);
        };
        let state = self.get_or_create_state(module, table, PersistenceKind::Logged)?;
        let log_path = self.log_path(root, module, table);
        let mut guard = state.lock();
        guard.persistence = PersistenceKind::Logged;
        let seq = guard.next_seq;
        guard.next_seq += 1;

        let entry = TableLogEntry::new(seq, operation);
        // Write without fsync — background WAL thread fsyncs every 10ms.
        self.write_log_entry_async(&log_path, &entry, module)?;

        let needs_snapshot = seq.saturating_sub(guard.last_snapshot_seq) >= SNAPSHOT_INTERVAL;
        if needs_snapshot {
            Ok(Some(SnapshotPlan {
                module: module.to_string(),
                table: table.to_string(),
                seq,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn snapshot_logged_table(
        &self,
        plan: SnapshotPlan,
        rows: Vec<Row>,
    ) -> Result<(), IntersticeError> {
        let Some(root) = &self.modules_root else {
            return Ok(());
        };

        // Flush WAL before writing snapshot so log is durable through this seq.
        self.flush_wal();

        let state = self.get_or_create_state(&plan.module, &plan.table, PersistenceKind::Logged)?;
        let module_paths = self.ensure_module_dirs(root, &plan.module)?;
        let snapshot_path = module_paths.snapshots.join(format!("{}.snap", plan.table));
        let log_path = module_paths.logs.join(format!("{}.log", plan.table));

        {
            let mut guard = state.lock();
            Self::write_snapshot_file(&snapshot_path, plan.seq, &rows)?;
            // compact_log replaces the file; drop the stale WalWriter so it reopens.
            Self::compact_log(&log_path, plan.seq)?;
            self.wal_writers.lock().remove(&log_path);
            guard.last_snapshot_seq = plan.seq;
        }

        Ok(())
    }

    /// Flush all pending WAL writes to disk.
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
            let paths: Vec<PathBuf> = writers
                .values()
                .filter(|w| w.dirty)
                .map(|w| w.path.clone())
                .collect();
            for writer in writers.values_mut() {
                writer.dirty = false;
            }
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

        for (key, row_opt) in batch {
            let dir = Self::stateful_dir(root, &key.module, &key.table);
            let _ = fs::create_dir_all(&dir);
            let path = dir.join(format!("{}.row", Self::pk_filename(&key.pk)));
            match row_opt {
                Some(row) => {
                    if let Ok(encoded) = encode(&row) {
                        let tmp = path.with_extension("row.tmp");
                        if let Ok(mut f) = File::create(&tmp) {
                            let _ = f.write_all(&encoded);
                            let _ = fs::rename(&tmp, &path);
                        }
                    }
                }
                None => {
                    let _ = fs::remove_file(&path);
                }
            }
        }
    }

    pub fn restore_table(&self, module: &str, table: &mut Table) -> Result<(), IntersticeError> {
        let Some(root) = &self.modules_root else {
            return Ok(());
        };

        if table.schema.persistence == PersistenceKind::Ephemeral {
            let state =
                self.get_or_create_state(module, &table.schema.name, PersistenceKind::Ephemeral)?;
            let mut guard = state.lock();
            guard.persistence = PersistenceKind::Ephemeral;
            guard.last_snapshot_seq = 0;
            guard.next_seq = 0;
            return Ok(());
        }

        // Stateful: restore from per-row files (no WAL)
        if table.schema.persistence == PersistenceKind::Stateful {
            let dir = Self::stateful_dir(root, module, &table.schema.name);
            if dir.exists() {
                for entry in fs::read_dir(&dir).map_err(|e| {
                    IntersticeError::Internal(format!("Failed to read stateful dir: {}", e))
                })? {
                    let entry = entry.map_err(|e| {
                        IntersticeError::Internal(format!("Failed to read dir entry: {}", e))
                    })?;
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("row") {
                        let bytes = fs::read(&path).map_err(|e| {
                            IntersticeError::Internal(format!("Failed to read row file: {}", e))
                        })?;
                        let row: Row = decode(&bytes).map_err(|e| {
                            IntersticeError::Internal(format!("Failed to decode row: {}", e))
                        })?;
                        table.insert(row)?;
                    }
                }
            }
            let state = self.get_or_create_state(module, &table.schema.name, PersistenceKind::Stateful)?;
            let mut guard = state.lock();
            guard.persistence = PersistenceKind::Stateful;
            return Ok(());
        }

        // Logged: snapshot + WAL replay
        let table_name = table.schema.name.clone();
        let module_paths = self.ensure_module_dirs(root, module)?;
        let snapshot_path = module_paths.snapshots.join(format!("{}.snap", table_name));
        let log_path = module_paths.logs.join(format!("{}.log", table_name));
        let snapshot = Self::read_snapshot_file(&snapshot_path)?;

        table.restore_from_rows(snapshot.rows)?;
        let mut last_seq = snapshot.last_seq;

        Self::read_log_entries(&log_path, |entry| {
            if entry.seq > snapshot.last_seq {
                TableStore::apply_entry(table, &entry.operation)?;
                last_seq = entry.seq;
            }
            Ok(())
        })?;

        let state =
            self.get_or_create_state(module, &table_name, table.schema.persistence.clone())?;
        let mut guard = state.lock();
        guard.persistence = table.schema.persistence.clone();
        guard.last_snapshot_seq = last_seq;
        guard.next_seq = last_seq.saturating_add(1);

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

    fn ensure_module_dirs(
        &self,
        root: &Path,
        module: &str,
    ) -> Result<ModulePaths, IntersticeError> {
        let module_dir = root.join(module);
        let logs = module_dir.join("logs");
        let snapshots = module_dir.join("snapshots");
        fs::create_dir_all(&logs).map_err(|err| {
            IntersticeError::Internal(format!(
                "Failed to create logs dir for module {}: {}",
                module, err
            ))
        })?;
        fs::create_dir_all(&snapshots).map_err(|err| {
            IntersticeError::Internal(format!(
                "Failed to create snapshots dir for module {}: {}",
                module, err
            ))
        })?;
        Ok(ModulePaths { logs, snapshots })
    }

    fn log_path(&self, root: &Path, module: &str, table: &str) -> PathBuf {
        root.join(module)
            .join("logs")
            .join(format!("{}.log", table))
    }

    /// Write a log entry to disk without fsync (async WAL hot path).
    /// Reuses an open file handle; opens one on first write to this path.
    fn write_log_entry_async(
        &self,
        path: &PathBuf,
        entry: &TableLogEntry,
        module: &str,
    ) -> Result<(), IntersticeError> {
        let encoded = encode(entry).map_err(|err| {
            IntersticeError::Internal(format!("Failed to encode log entry: {err}"))
        })?;
        let length = (encoded.len() as u32).to_le_bytes();

        // Fast path: reuse the already-open file handle.
        {
            let mut writers = self.wal_writers.lock();
            if let Some(writer) = writers.get_mut(path) {
                writer.file.write_all(&length).map_err(|err| {
                    IntersticeError::Internal(format!("Failed to write log length: {err}"))
                })?;
                writer.file.write_all(&encoded).map_err(|err| {
                    IntersticeError::Internal(format!("Failed to write log entry: {err}"))
                })?;
                writer.dirty = true;
                return Ok(());
            }
        }

        // Slow path: first write to this path — open the file.
        if let Some(root) = &self.modules_root {
            self.ensure_module_dirs(root, module)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|err| {
                IntersticeError::Internal(format!(
                    "Failed to open log file {:?}: {}",
                    path, err
                ))
            })?;

        let mut writers = self.wal_writers.lock();
        let writer = writers
            .entry(path.clone())
            .or_insert(WalWriter { file, path: path.clone(), dirty: false });
        writer.file.write_all(&length).map_err(|err| {
            IntersticeError::Internal(format!("Failed to write log length: {err}"))
        })?;
        writer.file.write_all(&encoded).map_err(|err| {
            IntersticeError::Internal(format!("Failed to write log entry: {err}"))
        })?;
        writer.dirty = true;
        Ok(())
    }

    fn read_log_entries<F>(path: &Path, mut visitor: F) -> Result<(), IntersticeError>
    where
        F: FnMut(TableLogEntry) -> Result<(), IntersticeError>,
    {
        if !path.exists() {
            return Ok(());
        }

        let mut file = File::open(path).map_err(|err| {
            IntersticeError::Internal(format!("Failed to open log file {:?}: {}", path, err))
        })?;
        file.seek(SeekFrom::Start(0))
            .map_err(|err| IntersticeError::Internal(format!("Failed to seek log file: {err}")))?;

        loop {
            let mut len_buf = [0u8; 4];
            if file.read_exact(&mut len_buf).is_err() {
                break;
            }
            let length = u32::from_le_bytes(len_buf) as usize;
            let mut buffer = vec![0u8; length];
            file.read_exact(&mut buffer).map_err(|err| {
                IntersticeError::Internal(format!("Failed to read log entry: {err}"))
            })?;
            let entry: TableLogEntry = decode(&buffer).map_err(|err| {
                IntersticeError::Internal(format!("Failed to decode log entry: {err}"))
            })?;
            visitor(entry)?;
        }

        Ok(())
    }

    fn write_snapshot_file(path: &Path, seq: u64, rows: &[Row]) -> Result<(), IntersticeError> {
        let snapshot = TableSnapshot {
            version: SNAPSHOT_VERSION,
            last_seq: seq,
            rows: rows.to_vec(),
        };
        let encoded = encode(&snapshot).map_err(|err| {
            IntersticeError::Internal(format!("Failed to encode snapshot: {err}"))
        })?;
        let tmp_path = path.with_extension("snap.tmp");
        {
            let mut file = File::create(&tmp_path).map_err(|err| {
                IntersticeError::Internal(format!(
                    "Failed to create snapshot temp file {:?}: {}",
                    tmp_path, err
                ))
            })?;
            file.write_all(&encoded).map_err(|err| {
                IntersticeError::Internal(format!("Failed to write snapshot: {err}"))
            })?;
            file.sync_all().map_err(|err| {
                IntersticeError::Internal(format!("Failed to sync snapshot: {err}"))
            })?;
        }
        fs::rename(&tmp_path, path).map_err(|err| {
            IntersticeError::Internal(format!("Failed to finalize snapshot {:?}: {}", path, err))
        })?;
        Ok(())
    }

    fn read_snapshot_file(path: &Path) -> Result<TableSnapshot, IntersticeError> {
        if !path.exists() {
            return Ok(TableSnapshot {
                version: SNAPSHOT_VERSION,
                last_seq: 0,
                rows: Vec::new(),
            });
        }
        let bytes = fs::read(path).map_err(|err| {
            IntersticeError::Internal(format!("Failed to read snapshot {:?}: {}", path, err))
        })?;
        decode(&bytes)
            .map_err(|err| IntersticeError::Internal(format!("Failed to decode snapshot: {err}")))
    }

    fn compact_log(path: &Path, keep_after_seq: u64) -> Result<(), IntersticeError> {
        if !path.exists() {
            return Ok(());
        }
        let tmp_path = path.with_extension("log.tmp");
        let mut reader = File::open(path).map_err(|err| {
            IntersticeError::Internal(format!("Failed to open log file {:?}: {}", path, err))
        })?;
        let mut writer = File::create(&tmp_path).map_err(|err| {
            IntersticeError::Internal(format!("Failed to create temp log: {err}"))
        })?;

        loop {
            let mut len_buf = [0u8; 4];
            if reader.read_exact(&mut len_buf).is_err() {
                break;
            }
            let length = u32::from_le_bytes(len_buf) as usize;
            let mut buffer = vec![0u8; length];
            reader.read_exact(&mut buffer).map_err(|err| {
                IntersticeError::Internal(format!("Failed to read log entry: {err}"))
            })?;
            let entry: TableLogEntry = decode(&buffer).map_err(|err| {
                IntersticeError::Internal(format!("Failed to decode log entry: {err}"))
            })?;
            if entry.seq > keep_after_seq {
                writer.write_all(&len_buf).map_err(|err| {
                    IntersticeError::Internal(format!("Failed to write compacted log: {err}"))
                })?;
                writer.write_all(&buffer).map_err(|err| {
                    IntersticeError::Internal(format!("Failed to write compacted log: {err}"))
                })?;
            }
        }

        writer.sync_all().map_err(|err| {
            IntersticeError::Internal(format!("Failed to sync compacted log: {err}"))
        })?;
        fs::rename(&tmp_path, path).map_err(|err| {
            IntersticeError::Internal(format!("Failed to replace log file: {err}"))
        })?;
        Ok(())
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
            LogOperation::Clear => {
                table.clear();
            }
        }
        Ok(())
    }

    pub fn forget_module(&self, module: &str) {
        self.cleanup_module(module);
    }
}

struct ModulePaths {
    logs: PathBuf,
    snapshots: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct TableLogEntry {
    seq: u64,
    timestamp_ms: u64,
    operation: LogOperation,
}

impl TableLogEntry {
    fn new(seq: u64, operation: LogOperation) -> Self {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Self {
            seq,
            timestamp_ms,
            operation,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct TableSnapshot {
    version: u16,
    last_seq: u64,
    rows: Vec<Row>,
}
