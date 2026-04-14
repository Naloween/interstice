use crate::{
    error::IntersticeError,
    runtime::transaction::Transaction,
    runtime::{Runtime, module::Module, table::TableAutoIncSnapshot},
};
use interstice_abi::{IntersticeValue, QuerySchema, RawReducerContext as ReducerContext, ReducerTableRef};
use serde::Serialize;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

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
    pub completion: Option<CompletionToken>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CallFrameKind {
    Reducer,
    Query,
}

#[derive(Debug)]
pub struct CallFrame {
    pub module: String,
    pub reducer: String,
    pub module_arc: Arc<Module>,
    pub kind: CallFrameKind,
    pub transactions: Vec<Transaction>,
    pub auto_inc_snapshots: HashMap<String, TableAutoIncSnapshot>,
    pub rng_state: u64,
    pub table_access: ReducerTableAccess,
}

impl CallFrame {
    pub fn new(
        module: String,
        reducer: String,
        module_arc: Arc<Module>,
        kind: CallFrameKind,
        rng_state: u64,
        table_access: ReducerTableAccess,
    ) -> Self {
        Self {
            module,
            reducer,
            module_arc,
            kind,
            transactions: Vec::new(),
            auto_inc_snapshots: HashMap::new(),
            rng_state,
            table_access,
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
    pub(crate) fn call_reducer(
        &self,
        module_name: &str,
        reducer_name: &str,
        args: impl Serialize,
        caller_node_id: crate::node::NodeId,
    ) -> Result<(), IntersticeError> {
        // ── Preamble: module lookup + cycle check + frame push ───────────────
        let module = {
            let mut modules = self.modules.lock();
            modules
                .get_mut(module_name)
                .ok_or_else(|| {
                    IntersticeError::ModuleNotFound(
                        module_name.into(),
                        format!(
                            "When trying to invoke reducer '{}' from '{}'",
                            reducer_name, module_name
                        ),
                    )
                })?
                .clone()
        };

        // Check that reducer exists in schema (O(1) HashSet lookup)
        if !module.reducer_names.contains(reducer_name) {
            return Err(IntersticeError::ReducerNotFound {
                module: module_name.into(),
                reducer: reducer_name.into(),
            });
        }

        // Detect cycles — check the current thread's stack only.
        let cycle = CALL_STACK.with(|s| s.borrow().iter().any(|f| f.module == module_name));
        if cycle {
            return Err(IntersticeError::ReducerCycle {
                module: module_name.into(),
                reducer: reducer_name.into(),
            });
        }

        // Push frame onto current thread's stack (no lock needed — TLS).
        let reducer_schema = module
            .schema
            .reducers
            .iter()
            .find(|r| r.name == reducer_name)
            .ok_or_else(|| IntersticeError::ReducerNotFound {
                module: module_name.into(),
                reducer: reducer_name.into(),
            })?;
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

        CALL_STACK.with(|s| {
            s.borrow_mut().push(CallFrame::new(
                module_name.into(),
                reducer_name.into(),
                module.clone(),
                CallFrameKind::Reducer,
                rng_seed,
                table_access,
            ));
        });

        // ── WASM execution ───────────────────────────────────────────────────
        let reducer_context = ReducerContext::new(caller_node_id.to_string());
        let call_result = module.call_reducer(reducer_name, (reducer_context, args));
        // Pop frame from current thread's stack; remove the entry when stack becomes empty.
        let reducer_frame = CALL_STACK.with(|s| s.borrow_mut().pop().unwrap());

        call_result?;

        // ── Transaction apply ────────────────────────────────────────────────
        let emitted_events = self.apply_all_transactions(reducer_frame.transactions, &module)?;

        // ── Event dispatch ───────────────────────────────────────────────────
        // For each event, fork the active completion token (if any) so the
        // render-done signal stays open until the event is fully processed by
        // the async event loop and any ReducerJobs it dispatches also complete.
        for ev in emitted_events {
            let token = ACTIVE_COMPLETION.with(|c| c.borrow().as_ref().map(|t| t.fork()));
            self.event_sender.send((ev, token)).unwrap();
        }

        Ok(())
    }
}
