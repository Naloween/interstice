use crate::{
    error::IntersticeError,
    runtime::transaction::Transaction,
    runtime::event::EventInstance,
    runtime::{Runtime, module::Module, table::{TableAutoIncSnapshot, TableOverlay}},
};
use interstice_abi::{IntersticeValue, QuerySchema, RawReducerContext as ReducerContext, ReducerTableRef};
use serde::Serialize;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

use crate::network::protocol::NetworkPacket;
use crate::runtime::blob::BlobScope;
use crate::runtime::table::Table;

/// A pinned read lock on one module's tables, held by the outermost query
/// frame until it returns (DESIGN §6.6, Snapshots).
pub type TablesPin =
    parking_lot::ArcRwLockReadGuard<parking_lot::RawRwLock, HashMap<String, Table>>;

thread_local! {
    /// Per-thread call stack.  No locking needed — each wasm-reducer thread and
    /// each tokio spawn_blocking task has its own copy.
    pub(crate) static CALL_STACK: RefCell<Vec<CallFrame>> = RefCell::new(Vec::new());

    /// Active completion token for the currently-running reducer job.  When a
    /// reducer dispatches downstream events, each new job gets a `fork()` so the
    /// render-done signal is held open until the full cascade completes.
    pub(crate) static ACTIVE_COMPLETION: RefCell<Option<CompletionToken>> = RefCell::new(None);
}

#[derive(Debug)]
pub struct ReducerJob {
    pub module_name: String,
    pub reducer_name: String,
    pub input: IntersticeValue,
    pub caller_node_id: crate::node::NodeId,
    /// Schema name of the calling module, or empty for runtime-originated jobs
    /// (events, render, network). See [`interstice_abi::RawReducerContext`].
    pub caller_module_name: String,
    pub completion: Option<CompletionToken>,
}

/// What the scheduler runs (DESIGN §6.6, Jobs).
#[derive(Debug)]
pub enum Job {
    Reducer(ReducerJob),
    /// The deferred local calls of one committed transaction.
    Calls {
        batch: CallBatch,
        completion: Option<CompletionToken>,
    },
    /// A remote table change to apply to the local replicas of that table.
    Replica {
        change: ReplicaChange,
        completion: Option<CompletionToken>,
    },
}

impl From<ReducerJob> for Job {
    fn from(job: ReducerJob) -> Self {
        Job::Reducer(job)
    }
}

/// A reducer call recorded by a transaction, run after it commits.
#[derive(Debug, Clone)]
pub struct PendingCall {
    pub module_name: String,
    pub reducer_name: String,
    pub input: IntersticeValue,
}

/// The local calls of one commit, run in issue order on one worker. Holds the
/// caller's blob scope so `Blob` arguments stay valid until the batch has run.
#[derive(Debug, Clone)]
pub struct CallBatch {
    pub caller_module_name: String,
    pub caller_node_id: crate::node::NodeId,
    pub calls: Vec<PendingCall>,
    pub scope: Arc<BlobScope>,
}

/// One entry of a transaction's outbox.
#[derive(Debug)]
pub enum OutboxEntry {
    Local(PendingCall),
    /// Blobs already inlined; sent when the caller commits.
    Remote {
        node_id: crate::node::NodeId,
        packet: NetworkPacket,
    },
}

/// A change to a remote table that local modules replicate.
#[derive(Debug, Clone)]
pub enum ReplicaChange {
    Insert {
        source_node_id: crate::node::NodeId,
        module_name: String,
        table_name: String,
        row: interstice_abi::Row,
    },
    Update {
        source_node_id: crate::node::NodeId,
        module_name: String,
        table_name: String,
        old_row: interstice_abi::Row,
        new_row: interstice_abi::Row,
    },
    Delete {
        source_node_id: crate::node::NodeId,
        module_name: String,
        table_name: String,
        row: interstice_abi::Row,
    },
    FullSync {
        source_node_id: crate::node::NodeId,
        module_name: String,
        table_name: String,
        rows: Vec<interstice_abi::Row>,
    },
}

impl ReplicaChange {
    pub fn source(&self) -> (crate::node::NodeId, &str, &str) {
        match self {
            ReplicaChange::Insert { source_node_id, module_name, table_name, .. }
            | ReplicaChange::Update { source_node_id, module_name, table_name, .. }
            | ReplicaChange::Delete { source_node_id, module_name, table_name, .. }
            | ReplicaChange::FullSync { source_node_id, module_name, table_name, .. } => {
                (*source_node_id, module_name, table_name)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CallFrameKind {
    Reducer,
    Query,
}

pub struct CallFrame {
    pub module: String,
    pub reducer: String,
    pub module_arc: Arc<Module>,
    pub kind: CallFrameKind,
    pub transactions: Vec<Transaction>,
    /// Read-your-writes view of `transactions`, per table of `module`.
    pub overlays: HashMap<String, TableOverlay>,
    pub auto_inc_snapshots: HashMap<String, TableAutoIncSnapshot>,
    pub rng_state: u64,
    pub table_access: ReducerTableAccess,
    /// Blobs of this call (shared with the caller for query frames).
    pub scope: Arc<BlobScope>,
    /// Calls issued by this transaction, released only if it commits.
    /// Only the root reducer frame uses it.
    pub outbox: Vec<OutboxEntry>,
    /// Read locks pinned by the outermost query frame, per module.
    pub pins: HashMap<String, TablesPin>,
}

impl CallFrame {
    pub fn new(
        module: String,
        reducer: String,
        module_arc: Arc<Module>,
        kind: CallFrameKind,
        rng_state: u64,
        table_access: ReducerTableAccess,
        scope: Arc<BlobScope>,
    ) -> Self {
        Self {
            module,
            reducer,
            module_arc,
            kind,
            transactions: Vec::new(),
            overlays: HashMap::new(),
            auto_inc_snapshots: HashMap::new(),
            rng_state,
            table_access,
            scope,
            outbox: Vec::new(),
            pins: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ReducerTableAccess {
    pub reads: HashSet<ReducerTableRef>,
    pub inserts: HashSet<ReducerTableRef>,
    pub updates: HashSet<ReducerTableRef>,
    pub deletes: HashSet<ReducerTableRef>,
}

impl ReducerTableAccess {
    pub fn from_schema(schema: &interstice_abi::ReducerSchema) -> Self {
        Self {
            reads: schema.reads.iter().cloned().collect(),
            inserts: schema.inserts.iter().cloned().collect(),
            updates: schema.updates.iter().cloned().collect(),
            deletes: schema.deletes.iter().cloned().collect(),
        }
    }

    pub fn from_query_schema(schema: &QuerySchema) -> Self {
        Self {
            reads: schema.reads.iter().cloned().collect(),
            inserts: HashSet::new(),
            updates: HashSet::new(),
            deletes: HashSet::new(),
        }
    }
}

/// A reference-counted completion token.  Multiple jobs that are causally
/// connected (a reducer that dispatches downstream events) share the same
/// token.  The oneshot fires only when the last token clone is dropped —
/// i.e., when the entire cascade of triggered reducers has finished.
#[derive(Clone, Debug)]
pub struct CompletionToken {
    count: Arc<AtomicUsize>,
    sender: Arc<Mutex<Option<oneshot::Sender<()>>>>,
}

impl CompletionToken {
    /// Create a fresh token (count = 1) and its paired receiver.
    pub fn new() -> (Self, oneshot::Receiver<()>) {
        let (tx, rx) = oneshot::channel();
        let token = Self {
            count: Arc::new(AtomicUsize::new(1)),
            sender: Arc::new(Mutex::new(Some(tx))),
        };
        (token, rx)
    }

    /// Increment the count and return a new token that shares the same latch.
    /// Call this when dispatching a downstream job so the signal is held open
    /// until that job (and anything it triggers) also completes.
    pub fn fork(&self) -> Self {
        self.count.fetch_add(1, Ordering::AcqRel);
        Self {
            count: Arc::clone(&self.count),
            sender: Arc::clone(&self.sender),
        }
    }
}

impl Drop for CompletionToken {
    fn drop(&mut self) {
        // Decrement; if we were the last holder, fire the signal.
        if self.count.fetch_sub(1, Ordering::AcqRel) == 1 {
            if let Ok(mut guard) = self.sender.lock() {
                if let Some(tx) = guard.take() {
                    let _ = tx.send(());
                }
            }
        }
    }
}

impl Runtime {
    /// Run one reducer transaction at top level (DESIGN §6.6): execute, commit,
    /// then release its outbox. `parent_scope` is the blob scope the job
    /// inherited (the caller's, for a call-batch callee).
    pub(crate) fn call_reducer(
        &self,
        module_name: &str,
        reducer_name: &str,
        args: impl Serialize,
        caller_node_id: crate::node::NodeId,
        caller_module_name: &str,
        parent_scope: Option<Arc<BlobScope>>,
    ) -> Result<(), IntersticeError> {
        let module = self.modules.lock().get(module_name).cloned().ok_or_else(|| {
            IntersticeError::ModuleNotFound(
                module_name.into(),
                format!("When trying to invoke reducer '{reducer_name}'"),
            )
        })?;
        let reducer_schema = module
            .schema
            .reducers
            .iter()
            .find(|r| r.name == reducer_name)
            .ok_or_else(|| IntersticeError::ReducerNotFound {
                module: module_name.into(),
                reducer: reducer_name.into(),
            })?;
        debug_assert!(
            CALL_STACK.with(|s| s.borrow().is_empty()),
            "reducers only run at top level"
        );
        let table_access = ReducerTableAccess::from_schema(reducer_schema);
        let call_sequence = self
            .call_sequence
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let rng_seed = crate::runtime::deterministic_random::seed_from_call(
            &caller_node_id,
            module_name,
            reducer_name,
            CallFrameKind::Reducer,
            call_sequence,
        );
        let scope = BlobScope::new(parent_scope);

        CALL_STACK.with(|s| {
            s.borrow_mut().push(CallFrame::new(
                module_name.into(),
                reducer_name.into(),
                module.clone(),
                CallFrameKind::Reducer,
                rng_seed,
                table_access,
                scope,
            ));
        });

        // ── WASM execution ───────────────────────────────────────────────────
        let reducer_context = ReducerContext::new(
            caller_node_id.to_string(),
            caller_module_name.to_string(),
            module.state_epoch(),
        );
        let call_result = module.call_reducer(reducer_name, (reducer_context, args));
        let frame = CALL_STACK.with(|s| s.borrow_mut().pop().unwrap());
        // On error the outbox is dropped with the writes.
        call_result?;

        // ── Commit, then release the outbox in commit order ──────────────────
        let mut locals = Vec::new();
        let mut remotes = Vec::new();
        for entry in frame.outbox {
            match entry {
                OutboxEntry::Local(call) => locals.push(call),
                OutboxEntry::Remote { node_id, packet } => remotes.push((node_id, packet)),
            }
        }
        let calls = (!locals.is_empty()).then(|| {
            EventInstance::Calls(CallBatch {
                caller_module_name: module_name.into(),
                caller_node_id: self.network_handle.node_id,
                calls: locals,
                scope: frame.scope.clone(),
            })
        });
        self.apply_all_transactions(frame.transactions, &frame.overlays, &module, calls)
            .inspect_err(|_| {
                // The instance ran to completion and may have cached what it
                // just wrote; those writes are now discarded.
                module.bump_state_epoch();
            })?;
        for (node_id, packet) in remotes {
            self.network_handle.send_packet(node_id, packet);
        }
        Ok(())
    }

    /// Push `event` into the event channel with a fork of the active
    /// completion token, so render-done waits for whatever it triggers.
    pub(crate) fn emit_event(&self, event: EventInstance) {
        let token = ACTIVE_COMPLETION.with(|c| c.borrow().as_ref().map(|t| t.fork()));
        let _ = self.event_sender.send((event, token));
    }

    /// Run a call batch: each call is its own top-level transaction, in issue
    /// order. A failing callee is logged and the batch continues.
    pub(crate) fn run_call_batch(&self, batch: CallBatch) {
        for call in batch.calls {
            if let Err(err) = self.call_reducer(
                &call.module_name,
                &call.reducer_name,
                call.input,
                batch.caller_node_id,
                &batch.caller_module_name,
                Some(batch.scope.clone()),
            ) {
                self.logger.log(
                    &format!(
                        "Reducer '{}.{}' called by '{}' failed: {}",
                        call.module_name, call.reducer_name, batch.caller_module_name, err
                    ),
                    crate::logger::LogSource::Runtime,
                    crate::logger::LogLevel::Error,
                );
            }
        }
    }
}
