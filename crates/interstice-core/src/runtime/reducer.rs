use crate::{
    error::IntersticeError,
    logger::{LogLevel, LogSource},
    runtime::transaction::Transaction,
    runtime::{Runtime, module::Module, table::TableAutoIncSnapshot},
};
use interstice_abi::{IntersticeValue, ReducerContext};
use serde::Serialize;
use std::cell::RefCell;
use std::collections::HashMap;
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

/// How many successful reducer calls between each perf report.
pub(crate) const PERF_PRINT_EVERY: u64 = 10_000;

#[derive(Debug)]
pub struct ReducerJob {
    pub module_name: String,
    pub reducer_name: String,
    pub input: IntersticeValue,
    pub caller_node_id: crate::node::NodeId,
    pub completion: Option<CompletionToken>,
    /// Timestamp recorded the moment this job was pushed into the reducer channel.
    pub queued_at: std::time::Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CallFrameKind {
    Reducer,
    Query,
}

#[derive(Debug)]
pub struct CallFrame {
    pub module: String,
    pub module_arc: Arc<Module>,
    pub kind: CallFrameKind,
    pub transactions: Vec<Transaction>,
    pub auto_inc_snapshots: HashMap<String, TableAutoIncSnapshot>,
    pub rng_state: u64,
}

impl CallFrame {
    pub fn new(
        module: String,
        module_arc: Arc<Module>,
        kind: CallFrameKind,
        rng_state: u64,
    ) -> Self {
        Self {
            module,
            module_arc,
            kind,
            transactions: Vec::new(),
            auto_inc_snapshots: HashMap::new(),
            rng_state,
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

/// Accumulated timing stats for the reducer pipeline. Reset after each report.
#[derive(Default)]
pub(crate) struct PerfStats {
    /// Number of successful calls included in this window.
    pub count: u64,
    /// Time from job enqueue to start of execution (queue back-pressure).
    pub queue_ns: u64,
    /// Module lookup + call-stack cycle check + frame push.
    pub preamble_ns: u64,
    /// WASM module.call_reducer() execution.
    pub wasm_ns: u64,
    /// apply_transaction() loop (in-memory table mutation + persistence writes).
    pub transaction_ns: u64,
    /// event_sender.send() loop (subscription fanout).
    pub events_ns: u64,
    /// preamble + wasm + transactions + events (excludes queue wait).
    pub total_ns: u64,
    /// Time from end of previous call to start of this call (recv + pool wait).
    pub inter_job_ns: u64,
}

impl PerfStats {
    pub fn record(
        &mut self,
        queue_ns: u64,
        preamble_ns: u64,
        wasm_ns: u64,
        transaction_ns: u64,
        events_ns: u64,
        inter_job_ns: u64,
    ) {
        self.count += 1;
        self.queue_ns += queue_ns;
        self.preamble_ns += preamble_ns;
        self.wasm_ns += wasm_ns;
        self.transaction_ns += transaction_ns;
        self.events_ns += events_ns;
        self.total_ns += preamble_ns + wasm_ns + transaction_ns + events_ns;
        self.inter_job_ns += inter_job_ns;
    }

    pub fn report(&self) -> String {
        let c = self.count.max(1) as f64;
        let us = |ns: u64| ns as f64 / c / 1_000.0;
        format!(
            "[Perf] reducer pipeline — avg over {} calls:\n\
             \x20 queue_wait    {:>9.2} µs  (time waiting in channel)\n\
             \x20 preamble      {:>9.2} µs  (module lookup + stack check + frame push)\n\
             \x20 wasm          {:>9.2} µs  (WASM execution)\n\
             \x20 transactions  {:>9.2} µs  (table mutation + persistence)\n\
             \x20 events        {:>9.2} µs  (subscription fanout)\n\
             \x20 total         {:>9.2} µs  (preamble + wasm + transactions + events)\n\
             \x20 inter_job     {:>9.2} µs  (recv + instance pool wait between jobs)",
            self.count,
            us(self.queue_ns),
            us(self.preamble_ns),
            us(self.wasm_ns),
            us(self.transaction_ns),
            us(self.events_ns),
            us(self.total_ns),
            us(self.inter_job_ns),
        )
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

impl Runtime {
    pub(crate) fn call_reducer(
        &self,
        module_name: &str,
        reducer_name: &str,
        args: impl Serialize,
        caller_node_id: crate::node::NodeId,
        queued_at: std::time::Instant,
        inter_job_ns: u64,
    ) -> Result<(), IntersticeError> {
        let t_start = std::time::Instant::now();
        let queue_ns = t_start.duration_since(queued_at).as_nanos() as u64;

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
                module.clone(),
                CallFrameKind::Reducer,
                rng_seed,
            ));
        });

        let t_preamble_done = std::time::Instant::now();

        // ── WASM execution ───────────────────────────────────────────────────
        let reducer_context = ReducerContext::new(caller_node_id.to_string());
        let call_result = module.call_reducer(reducer_name, (reducer_context, args));
        // Pop frame from current thread's stack; remove the entry when stack becomes empty.
        let reducer_frame = CALL_STACK.with(|s| s.borrow_mut().pop().unwrap());

        let t_wasm_done = std::time::Instant::now();

        call_result?;

        // ── Transaction apply ────────────────────────────────────────────────
        let emitted_events = self.apply_all_transactions(reducer_frame.transactions, &module)?;

        let t_transactions_done = std::time::Instant::now();

        // ── Event dispatch ───────────────────────────────────────────────────
        // For each event, fork the active completion token (if any) so the
        // render-done signal stays open until the event is fully processed by
        // the async event loop and any ReducerJobs it dispatches also complete.
        for ev in emitted_events {
            let token = ACTIVE_COMPLETION.with(|c| c.borrow().as_ref().map(|t| t.fork()));
            self.event_sender.send((ev, token)).unwrap();
        }

        let t_events_done = std::time::Instant::now();

        // ── Record and maybe print ───────────────────────────────────────────
        let preamble_ns = t_preamble_done.duration_since(t_start).as_nanos() as u64;
        let wasm_ns = t_wasm_done.duration_since(t_preamble_done).as_nanos() as u64;
        let transaction_ns = t_transactions_done.duration_since(t_wasm_done).as_nanos() as u64;
        let events_ns = t_events_done.duration_since(t_transactions_done).as_nanos() as u64;

        let mut perf = self.perf.lock();
        perf.record(
            queue_ns,
            preamble_ns,
            wasm_ns,
            transaction_ns,
            events_ns,
            inter_job_ns,
        );
        if perf.count % PERF_PRINT_EVERY == 0 {
            let report = perf.report();
            perf.reset();
            drop(perf);
            self.logger.log(&report, LogSource::Runtime, LogLevel::Info);
        }

        Ok(())
    }
}
