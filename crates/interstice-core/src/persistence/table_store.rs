use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use interstice_abi::{decode, encode, IndexKey, PersistenceKind, Row};
use serde::{Deserialize, Serialize};

use crate::{
    error::IntersticeError,
    runtime::table::Table,
};

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

pub struct TableStore {
    modules_root: Option<PathBuf>,
    tables: Mutex<HashMap<TableKey, Arc<Mutex<TableState>>>>,
}

impl TableStore {
    pub fn new(root: Option<PathBuf>) -> Self {
        Self {
            modules_root: root,
            tables: Mutex::new(HashMap::new()),
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
        let mut guard = state.lock().unwrap();
        guard.persistence = PersistenceKind::Logged;
        let seq = guard.next_seq;
        guard.next_seq += 1;

        let entry = TableLogEntry::new(seq, operation);
        Self::append_log_entry(&log_path, &entry)?;

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

        let state = self.get_or_create_state(&plan.module, &plan.table, PersistenceKind::Logged)?;
        let module_paths = self.ensure_module_dirs(root, &plan.module)?;
        let snapshot_path = module_paths.snapshots.join(format!("{}.snap", plan.table));
        let log_path = module_paths.logs.join(format!("{}.log", plan.table));

        {
            let mut guard = state.lock().unwrap();
            Self::write_snapshot_file(&snapshot_path, plan.seq, &rows)?;
            Self::compact_log(&log_path, plan.seq)?;
            guard.last_snapshot_seq = plan.seq;
        }

        Ok(())
    }

    pub fn persist_stateful_operation(
        &self,
        module: &str,
        table: &str,
        operation: LogOperation,
        rows: Vec<Row>,
    ) -> Result<(), IntersticeError> {
        let Some(root) = &self.modules_root else {
            return Ok(());
        };

        let state = self.get_or_create_state(module, table, PersistenceKind::Stateful)?;
        let module_paths = self.ensure_module_dirs(root, module)?;
        let snapshot_path = module_paths.snapshots.join(format!("{}.snap", table));
        let log_path = module_paths.logs.join(format!("{}.log", table));

        let mut guard = state.lock().unwrap();
        let seq = guard.next_seq;
        guard.next_seq += 1;

        Self::write_snapshot_file(&snapshot_path, seq, &rows)?;
        Self::append_log_entry(&log_path, &TableLogEntry::new(seq, operation))?;
        guard.last_snapshot_seq = seq;
        Ok(())
    }

    pub fn restore_table(
        &self,
        module: &str,
        table: &mut Table,
    ) -> Result<(), IntersticeError> {
        let Some(root) = &self.modules_root else {
            return Ok(());
        };

        let table_name = table.schema.name.clone();
        let module_paths = self.ensure_module_dirs(root, module)?;
        let snapshot_path = module_paths.snapshots.join(format!("{}.snap", table_name));
        let log_path = module_paths.logs.join(format!("{}.log", table_name));
        let snapshot = Self::read_snapshot_file(&snapshot_path)?;

        table.restore_from_rows(snapshot.rows)?;
        let mut last_seq = snapshot.last_seq;

        if table.schema.persistence != PersistenceKind::Stateful {
            Self::read_log_entries(&log_path, |entry| {
                if entry.seq > snapshot.last_seq {
                    TableStore::apply_entry(table, &entry.operation)?;
                    last_seq = entry.seq;
                }
                Ok(())
            })?;
        }

        let state = self.get_or_create_state(module, &table_name, table.schema.persistence.clone())?;
        let mut guard = state.lock().unwrap();
        guard.persistence = table.schema.persistence.clone();
        guard.last_snapshot_seq = last_seq;
        guard.next_seq = last_seq.saturating_add(1);

        Ok(())
    }

    pub fn clear_all(&self) -> Result<(), IntersticeError> {
        let Some(root) = &self.modules_root else {
            return Ok(());
        };

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
                    }
                }
            }
        }

        self.tables.lock().unwrap().clear();
        Ok(())
    }

    pub fn cleanup_module(&self, module: &str) {
        let mut tables = self.tables.lock().unwrap();
        tables.retain(|key, _| key.module != module);
    }

    fn get_or_create_state(
        &self,
        module: &str,
        table: &str,
        persistence: PersistenceKind,
    ) -> Result<Arc<Mutex<TableState>>, IntersticeError> {
        let mut tables = self.tables.lock().unwrap();
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
        root.join(module).join("logs").join(format!("{}.log", table))
    }

    fn append_log_entry(path: &Path, entry: &TableLogEntry) -> Result<(), IntersticeError> {
        let encoded = encode(entry).map_err(|err| {
            IntersticeError::Internal(format!("Failed to encode log entry: {err}"))
        })?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|err| {
                IntersticeError::Internal(format!("Failed to open log file {:?}: {}", path, err))
            })?;

        let length = (encoded.len() as u32).to_le_bytes();
        file.write_all(&length).map_err(|err| {
            IntersticeError::Internal(format!("Failed to write log length: {err}"))
        })?;
        file.write_all(&encoded).map_err(|err| {
            IntersticeError::Internal(format!("Failed to write log entry: {err}"))
        })?;
        file.sync_data().map_err(|err| {
            IntersticeError::Internal(format!("Failed to sync log file: {err}"))
        })?;
        Ok(())
    }

    fn read_log_entries<F>(
        path: &Path,
        mut visitor: F,
    ) -> Result<(), IntersticeError>
    where
        F: FnMut(TableLogEntry) -> Result<(), IntersticeError>,
    {
        if !path.exists() {
            return Ok(());
        }

        let mut file = File::open(path).map_err(|err| {
            IntersticeError::Internal(format!("Failed to open log file {:?}: {}", path, err))
        })?;
        file.seek(SeekFrom::Start(0)).map_err(|err| {
            IntersticeError::Internal(format!("Failed to seek log file: {err}"))
        })?;

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

    fn write_snapshot_file(
        path: &Path,
        seq: u64,
        rows: &[Row],
    ) -> Result<(), IntersticeError> {
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
            IntersticeError::Internal(format!(
                "Failed to finalize snapshot {:?}: {}",
                path, err
            ))
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
        decode(&bytes).map_err(|err| {
            IntersticeError::Internal(format!("Failed to decode snapshot: {err}"))
        })
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