//! Record and replay (DESIGN §6.7).
//!
//! A recorded module keeps a *baseline* copy of its tables and a log of every
//! commit into them since. A commit made by one of its reducers logs what
//! entered the module (input, `Tx`, the responses of host calls that cross
//! its boundary, blobs it read from its callers, auto-inc ids); any other
//! commit logs only its writes. Replay re-runs the log against a detached
//! copy of the module and checks it produces the same writes and state.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use interstice_abi::{HostCall, IntersticeValue, ModuleSelection, encode};

use crate::error::IntersticeError;
use crate::logger::{LogLevel, LogSource};
use crate::runtime::Runtime;
use crate::runtime::blob::{BlobData, BlobScope};
use crate::runtime::deterministic_random::{FNV_OFFSET, Tx, fnv1a};
use crate::runtime::module::Module;
use crate::runtime::reducer::{CALL_STACK, CallFrame, CallFrameKind, ReducerTableAccess};
use crate::runtime::table::Table;
use crate::runtime::transaction::Transaction;

/// Records kept before the oldest is folded into the baseline.
pub const DEFAULT_WINDOW: usize = 10_000;

/// Recording generations; 0 means "not recording".
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

/// The response to one host call that crossed the module boundary.
#[derive(Clone, Debug)]
pub struct LoggedCall {
    pub kind: &'static str,
    /// Digest of the encoded request.
    pub digest: u64,
    /// The encoded response the guest received, if any.
    pub response: Option<Vec<u8>>,
}

/// Everything a reducer transaction received from outside the module.
#[derive(Clone, Debug)]
pub struct ReducerRecord {
    pub reducer: String,
    /// Encoded `(ReducerContext, args)`.
    pub input: Vec<u8>,
    /// The `Tx` as it was when the transaction began.
    pub tx: Tx,
    pub calls: Vec<LoggedCall>,
    pub blobs: Vec<(u64, BlobData)>,
    /// Auto-inc values handed out, one entry per insert into an auto-inc table.
    pub auto_inc: Vec<Vec<IntersticeValue>>,
}

#[derive(Clone, Debug)]
pub enum Origin {
    Reducer(Box<ReducerRecord>),
    /// Writes that reached the tables without a recorded reducer: replica
    /// changes, and transactions that began before this recording did.
    External,
}

#[derive(Clone, Debug)]
pub struct CommitRecord {
    pub origin: Origin,
    pub writes: Vec<Transaction>,
}

pub struct Recorder {
    generation: u64,
    baseline: HashMap<String, Table>,
    log: VecDeque<CommitRecord>,
    window: usize,
}

/// How a root frame treats host calls that cross its module's boundary.
#[derive(Default)]
pub enum FrameMode {
    #[default]
    Live,
    Record(Box<RecordCapture>),
    Replay(Box<ReplayCursor>),
}

/// What a recorded transaction has received so far.
pub struct RecordCapture {
    pub generation: u64,
    pub calls: Vec<LoggedCall>,
    pub blobs: Vec<(u64, BlobData)>,
    pub auto_inc: Vec<Vec<IntersticeValue>>,
}

impl RecordCapture {
    pub fn new(generation: u64) -> Self {
        Self { generation, calls: Vec::new(), blobs: Vec::new(), auto_inc: Vec::new() }
    }
}

/// The logged inputs a replayed transaction has yet to consume.
pub struct ReplayCursor {
    pub calls: VecDeque<LoggedCall>,
    pub auto_inc: VecDeque<Vec<IntersticeValue>>,
    /// The first divergence met, reported instead of whatever the guest does
    /// with the error it got.
    pub divergence: Option<String>,
}

/// A committing reducer's capture, handed to the commit.
pub struct PendingOrigin {
    pub generation: u64,
    pub record: ReducerRecord,
}

impl PendingOrigin {
    pub fn new(capture: RecordCapture, reducer: &str, input: Vec<u8>, tx: Tx) -> Self {
        Self {
            generation: capture.generation,
            record: ReducerRecord {
                reducer: reducer.to_string(),
                input,
                tx,
                calls: capture.calls,
                blobs: capture.blobs,
                auto_inc: capture.auto_inc,
            },
        }
    }
}

// ── Module side ──────────────────────────────────────────────────────────────

impl Module {
    /// The current recording generation, 0 when not recording.
    pub fn recording_generation(&self) -> u64 {
        self.recording.load(Ordering::Acquire)
    }

    /// Start recording from the current state (restarting if already on).
    pub fn start_recording(&self, window: usize) {
        let tables = self.tables.write();
        let mut recorder = self.recorder.lock();
        let generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
        *recorder = Some(Recorder {
            generation,
            baseline: tables.iter().map(|(n, t)| (n.clone(), t.detached_clone())).collect(),
            log: VecDeque::new(),
            window: window.max(1),
        });
        self.recording.store(generation, Ordering::Release);
    }

    pub fn stop_recording(&self) {
        let _tables = self.tables.write();
        *self.recorder.lock() = None;
        self.recording.store(0, Ordering::Release);
    }

    /// Log a commit. The caller holds the table write lock and has applied
    /// `writes`, so log order is commit order.
    pub(crate) fn record_commit_locked(&self, origin: Option<PendingOrigin>, writes: Vec<Transaction>) {
        let mut guard = self.recorder.lock();
        let Some(recorder) = guard.as_mut() else { return };
        let origin = match origin {
            Some(o) if o.generation == recorder.generation => Origin::Reducer(Box::new(o.record)),
            _ => Origin::External,
        };
        recorder.log.push_back(CommitRecord { origin, writes });
        while recorder.log.len() > recorder.window {
            let oldest = recorder.log.pop_front().unwrap();
            if let Err(err) = apply_writes(&mut recorder.baseline, &oldest.writes) {
                // The baseline no longer rebuilds the live state.
                self.recording.store(0, Ordering::Release);
                *guard = None;
                self.runtime().log_record_event(
                    &format!("record: '{}' stopped, baseline update failed: {err}", self.schema.name),
                    true,
                );
                return;
            }
        }
    }
}

/// Apply committed `writes` to `tables` like a commit does, without
/// persistence or events.
pub(crate) fn apply_writes(
    tables: &mut HashMap<String, Table>,
    writes: &[Transaction],
) -> Result<(), IntersticeError> {
    fn table<'a>(
        tables: &'a mut HashMap<String, Table>,
        module_name: &str,
        table_name: &str,
    ) -> Result<&'a mut Table, IntersticeError> {
        tables.get_mut(table_name).ok_or_else(|| IntersticeError::TableNotFound {
            module_name: module_name.into(),
            table_name: table_name.into(),
        })
    }
    for write in writes {
        match write {
            Transaction::Insert { module_name, table_name, new_row } => {
                table(tables, module_name, table_name)?.insert_trusted(new_row.clone())?;
            }
            Transaction::Update { module_name, table_name, update_row } => {
                table(tables, module_name, table_name)?.update(update_row.clone())?;
            }
            Transaction::Delete { module_name, table_name, deleted_row_id } => {
                let _ = table(tables, module_name, table_name)?.delete(deleted_row_id);
            }
            Transaction::Clear { module_name, table_name } => {
                table(tables, module_name, table_name)?.clear();
            }
        }
    }
    for table in tables.values_mut() {
        table.enforce_retention();
    }
    Ok(())
}

// ── Host-call boundary ───────────────────────────────────────────────────────

thread_local! {
    /// Armed (`Some`) while a recorded host call runs: catches the response
    /// bytes sent to the guest. The last write wins, i.e. the outermost one.
    static RESPONSE_CAPTURE: RefCell<Option<Option<Vec<u8>>>> = const { RefCell::new(None) };
}

/// Called with every response written into a guest.
pub(crate) fn capture_response(bytes: &[u8]) {
    RESPONSE_CAPTURE.with(|c| {
        if let Some(slot) = c.borrow_mut().as_mut() {
            *slot = Some(bytes.to_vec());
        }
    });
}

/// Whether the current thread is replaying.
pub fn in_replay() -> bool {
    CALL_STACK.with(|s| {
        s.borrow()
            .first()
            .is_some_and(|f| matches!(f.mode, FrameMode::Replay(_)))
    })
}

pub(crate) enum Boundary {
    Record,
    Replay,
}

/// The boundary handling for a host call `call` made on this thread: only a
/// root frame that is recorded or replayed, and only for calls that leave
/// its module (§6.7).
pub(crate) fn boundary_for(call: &HostCall) -> Option<Boundary> {
    CALL_STACK.with(|s| {
        let stack = s.borrow();
        if stack.len() != 1 {
            return None;
        }
        let root = &stack[0];
        let boundary = match root.mode {
            FrameMode::Live => return None,
            FrameMode::Record(_) => Boundary::Record,
            FrameMode::Replay(_) => Boundary::Replay,
        };
        let own = |selection: &ModuleSelection| match selection {
            ModuleSelection::Current => true,
            ModuleSelection::Other(name) => *name == root.module,
        };
        let internal = match call {
            HostCall::TableScan(r) => own(&r.module_selection),
            HostCall::TableGetByPrimaryKey(r) => own(&r.module_selection),
            HostCall::TableIndexScan(r) => own(&r.module_selection),
            _ => false,
        };
        (!internal).then_some(boundary)
    })
}

pub(crate) fn host_call_kind(call: &HostCall) -> &'static str {
    match call {
        HostCall::CallReducer(_) => "call_reducer",
        HostCall::Schedule(_) => "schedule",
        HostCall::CallQuery(_) => "call_query",
        HostCall::TableScan(_) => "table_scan",
        HostCall::TableGetByPrimaryKey(_) => "table_get_by_primary_key",
        HostCall::TableIndexScan(_) => "table_index_scan",
        HostCall::Gpu(_) => "gpu",
        HostCall::Audio(_) => "audio",
        HostCall::File(_) => "file",
        HostCall::Module(_) => "module",
        HostCall::Network(_) => "network",
        HostCall::CurrentNodeId => "current_node_id",
    }
}

pub(crate) fn request_digest(request: &[u8]) -> u64 {
    fnv1a(FNV_OFFSET, request)
}

/// Run `perform` (the real host call) and log its response on the root frame.
pub(crate) fn record_call(
    kind: &'static str,
    digest: u64,
    perform: impl FnOnce() -> Result<Option<i64>, IntersticeError>,
) -> Result<Option<i64>, IntersticeError> {
    let previous = RESPONSE_CAPTURE.with(|c| c.borrow_mut().replace(None));
    let result = perform();
    let response = RESPONSE_CAPTURE.with(|c| std::mem::replace(&mut *c.borrow_mut(), previous)).flatten();
    CALL_STACK.with(|s| {
        if let Some(FrameMode::Record(capture)) = s.borrow_mut().first_mut().map(|f| &mut f.mode) {
            capture.calls.push(LoggedCall { kind, digest, response });
        }
    });
    result
}

/// The logged response for the next host call of a replayed root frame.
pub(crate) fn replay_call(kind: &'static str, digest: u64) -> Result<Option<Vec<u8>>, String> {
    CALL_STACK.with(|s| {
        let mut stack = s.borrow_mut();
        let Some(FrameMode::Replay(cursor)) = stack.first_mut().map(|f| &mut f.mode) else {
            return Err("not replaying".into());
        };
        let outcome = match cursor.calls.pop_front() {
            None => Err(format!("extra host call '{kind}' (not in the log)")),
            Some(logged) if logged.kind != kind => Err(format!(
                "host call '{kind}' where the log has '{}'",
                logged.kind
            )),
            Some(logged) if logged.digest != digest => {
                Err(format!("host call '{kind}' with a different request than logged"))
            }
            Some(logged) => Ok(logged.response),
        };
        if let Err(why) = &outcome {
            cursor.divergence.get_or_insert_with(|| why.clone());
        }
        outcome
    })
}

/// Auto-inc handling for an insert by the current frame: `None` to draw
/// fresh values (logging them when recording), or the logged values to use.
pub(crate) fn logged_auto_inc() -> Result<Option<Vec<IntersticeValue>>, String> {
    CALL_STACK.with(|s| {
        let mut stack = s.borrow_mut();
        match stack.first_mut().map(|f| &mut f.mode) {
            Some(FrameMode::Replay(cursor)) => match cursor.auto_inc.pop_front() {
                Some(values) => Ok(Some(values)),
                None => {
                    let why = "extra auto_inc insert (not in the log)".to_string();
                    cursor.divergence.get_or_insert_with(|| why.clone());
                    Err(why)
                }
            },
            _ => Ok(None),
        }
    })
}

/// Log the auto-inc values an insert drew, when recording.
pub(crate) fn note_auto_inc(values: impl FnOnce() -> Vec<IntersticeValue>) {
    CALL_STACK.with(|s| {
        if let Some(FrameMode::Record(capture)) = s.borrow_mut().first_mut().map(|f| &mut f.mode) {
            capture.auto_inc.push(values());
        }
    });
}

// ── Replay ───────────────────────────────────────────────────────────────────

impl Runtime {
    /// Replay `module_name`'s log against a detached copy (DESIGN §6.7).
    /// `Ok` is the report; `Err` the first divergence.
    pub(crate) fn replay_module(self: &Arc<Self>, module_name: &str) -> Result<String, String> {
        let module = self
            .modules
            .lock()
            .get(module_name)
            .cloned()
            .ok_or_else(|| format!("module '{module_name}' is not loaded"))?;

        // The log and the live state, as of the same commit.
        let (baseline, log, live) = {
            let tables = module.tables.read();
            let recorder = module.recorder.lock();
            let recorder = recorder
                .as_ref()
                .ok_or_else(|| format!("module '{module_name}' is not being recorded"))?;
            let baseline: HashMap<String, Table> = recorder
                .baseline
                .iter()
                .map(|(n, t)| (n.clone(), t.detached_clone()))
                .collect();
            let live: BTreeMap<String, Vec<interstice_abi::Row>> =
                tables.iter().map(|(n, t)| (n.clone(), t.snapshot_rows())).collect();
            (baseline, recorder.log.iter().cloned().collect::<Vec<_>>(), live)
        };

        let runtime = self.clone();
        let wasm = module.wasm_bytes().to_vec();
        let name = module_name.to_string();
        std::thread::Builder::new()
            .name(format!("replay-{module_name}"))
            .spawn(move || runtime.run_replay(&name, &wasm, baseline, log, live))
            .map_err(|e| format!("cannot start the replay thread: {e}"))?
            .join()
            .map_err(|_| "the replay thread panicked".to_string())?
    }

    fn run_replay(
        self: &Arc<Self>,
        module_name: &str,
        wasm: &[u8],
        baseline: HashMap<String, Table>,
        log: Vec<CommitRecord>,
        live: BTreeMap<String, Vec<interstice_abi::Row>>,
    ) -> Result<String, String> {
        let copy = Arc::new(
            Module::build(self.clone(), wasm).map_err(|e| format!("cannot build the copy: {e}"))?,
        );
        *copy.tables.write() = baseline;

        let mut reducers = 0usize;
        for (index, record) in log.iter().enumerate() {
            let at = |why: String| match &record.origin {
                Origin::Reducer(r) => format!("commit {index} (reducer '{}'): {why}", r.reducer),
                Origin::External => format!("commit {index} (external write): {why}"),
            };
            if let Origin::Reducer(r) = &record.origin {
                reducers += 1;
                let writes = self.replay_reducer(&copy, r).map_err(at)?;
                if let Some(why) = compare_writes(&writes, &record.writes) {
                    return Err(at(why));
                }
            }
            apply_writes(&mut copy.tables.write(), &record.writes)
                .map_err(|e| at(format!("logged writes no longer apply: {e}")))?;
        }

        let tables = copy.tables.read();
        for (table_name, live_rows) in &live {
            let replayed = tables.get(table_name).map(|t| t.scan()).unwrap_or(&[]);
            let same = |a: &interstice_abi::Row, b: &interstice_abi::Row| {
                encode(a).ok() == encode(b).ok()
            };
            let first_diff = (0..live_rows.len().max(replayed.len())).find(|&i| {
                match (live_rows.get(i), replayed.get(i)) {
                    (Some(a), Some(b)) => !same(a, b),
                    _ => true,
                }
            });
            if let Some(row) = first_diff {
                return Err(format!(
                    "final state differs: table '{table_name}' at row {row} \
                     ({} live rows, {} replayed)",
                    live_rows.len(),
                    replayed.len()
                ));
            }
        }
        Ok(format!(
            "replayed {} commits of '{module_name}' ({reducers} reducer, {} external): state matches",
            log.len(),
            log.len() - reducers
        ))
    }

    /// Run one logged reducer transaction on `copy` and return its writes.
    fn replay_reducer(&self, copy: &Arc<Module>, record: &ReducerRecord) -> Result<Vec<Transaction>, String> {
        let schema = copy
            .schema
            .reducers
            .iter()
            .find(|r| r.name == record.reducer)
            .ok_or("the reducer no longer exists")?;
        let scope = BlobScope::new(Some(BlobScope::with_blobs(record.blobs.iter().cloned())));
        let mut frame = CallFrame::new(
            copy.schema.name.clone(),
            record.reducer.clone(),
            copy.clone(),
            CallFrameKind::Reducer,
            record.tx,
            ReducerTableAccess::from_schema(schema),
            scope,
        );
        frame.mode = FrameMode::Replay(Box::new(ReplayCursor {
            calls: record.calls.iter().cloned().collect(),
            auto_inc: record.auto_inc.iter().cloned().collect(),
            divergence: None,
        }));
        CALL_STACK.with(|s| s.borrow_mut().push(frame));
        let result = copy.call_reducer_encoded(&record.reducer, &record.input);
        let frame = CALL_STACK.with(|s| s.borrow_mut().pop().unwrap());
        let FrameMode::Replay(cursor) = frame.mode else { unreachable!() };
        if let Some(why) = cursor.divergence {
            return Err(why);
        }
        result.map_err(|e| format!("failed on replay: {e}"))?;
        if let Some(next) = cursor.calls.front() {
            return Err(format!(
                "{} logged host call(s) not made (next: '{}')",
                cursor.calls.len(),
                next.kind
            ));
        }
        if !cursor.auto_inc.is_empty() {
            return Err(format!("{} logged auto_inc insert(s) not made", cursor.auto_inc.len()));
        }
        Ok(frame.transactions)
    }

    /// Node-log a record/replay outcome.
    pub(crate) fn log_record_event(&self, message: &str, error: bool) {
        self.logger.log(
            message,
            LogSource::Runtime,
            if error { LogLevel::Error } else { LogLevel::Info },
        );
    }
}

/// `None` if `replayed` equals `logged`, else why not.
fn compare_writes(replayed: &[Transaction], logged: &[Transaction]) -> Option<String> {
    let differs = |a: &Transaction, b: &Transaction| encode(a).ok() != encode(b).ok();
    for (i, (a, b)) in replayed.iter().zip(logged).enumerate() {
        if differs(a, b) {
            return Some(format!("write {i} differs: replayed {a:?}, logged {b:?}"));
        }
    }
    (replayed.len() != logged.len()).then(|| {
        format!(
            "{} writes replayed, {} logged",
            replayed.len(),
            logged.len()
        )
    })
}
