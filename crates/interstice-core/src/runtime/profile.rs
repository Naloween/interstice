//! Where a call's time goes (DESIGN §6.8): counters per phase, on when
//! `INTERSTICE_PROFILE` is set or [`enable`] was called, otherwise one
//! relaxed load per probe.
//!
//! Each phase keeps a count, a total and a maximum. Phases run on several
//! threads at once, so totals are summed CPU-side time, not wall time; the
//! report gives each phase's mean and its share of the total.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// In the scheduler, waiting for conflicting jobs to finish.
    Blocked,
    /// Ready, waiting for a free worker thread.
    Ready,
    /// Security check, call frame and argument encoding.
    Prepare,
    /// Waiting for a free WASM instance of the module.
    Instance,
    /// The guest call, host calls included.
    Wasm,
    /// Host calls made by the guest, by kind.
    HostInsert,
    HostUpdate,
    HostDelete,
    HostGet,
    HostScan,
    HostOther,
    /// Applying the writes: lock, re-validation, tables, events.
    Commit,
    /// Persistence work inside the commit (log and row files).
    Persist,
    /// Answering the caller.
    Reply,
}

const PHASES: [Phase; 14] = [
    Phase::Blocked,
    Phase::Ready,
    Phase::Prepare,
    Phase::Instance,
    Phase::Wasm,
    Phase::HostInsert,
    Phase::HostUpdate,
    Phase::HostDelete,
    Phase::HostGet,
    Phase::HostScan,
    Phase::HostOther,
    Phase::Commit,
    Phase::Persist,
    Phase::Reply,
];

impl Phase {
    fn label(self) -> &'static str {
        match self {
            Phase::Blocked => "blocked (scheduler)",
            Phase::Ready => "ready (worker)",
            Phase::Prepare => "prepare",
            Phase::Instance => "instance wait",
            Phase::Wasm => "wasm (incl. host)",
            Phase::HostInsert => "  host insert",
            Phase::HostUpdate => "  host update",
            Phase::HostDelete => "  host delete",
            Phase::HostGet => "  host get",
            Phase::HostScan => "  host scan",
            Phase::HostOther => "  host other",
            Phase::Commit => "commit",
            Phase::Persist => "  persist",
            Phase::Reply => "reply",
        }
    }
}

struct Counter {
    count: AtomicU64,
    total_ns: AtomicU64,
    max_ns: AtomicU64,
}

#[allow(clippy::declare_interior_mutable_const)]
const ZERO: Counter = Counter {
    count: AtomicU64::new(0),
    total_ns: AtomicU64::new(0),
    max_ns: AtomicU64::new(0),
};

static ENABLED: AtomicBool = AtomicBool::new(false);
static COUNTERS: [Counter; PHASES.len()] = [ZERO; PHASES.len()];
static SINCE: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);

/// Turn profiling on if `INTERSTICE_PROFILE` is set. Returns whether it is on.
pub fn init_from_env() -> bool {
    if std::env::var_os("INTERSTICE_PROFILE").is_some() {
        enable();
    }
    is_enabled()
}

/// Turn profiling on (and reset the counters).
pub fn enable() {
    reset();
    ENABLED.store(true, Ordering::Relaxed);
}

#[inline]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// The current time if profiling is on.
#[inline]
pub fn now() -> Option<Instant> {
    is_enabled().then(Instant::now)
}

#[inline]
pub fn record(phase: Phase, elapsed: Duration) {
    let c = &COUNTERS[phase as usize];
    let ns = elapsed.as_nanos().min(u64::MAX as u128) as u64;
    c.count.fetch_add(1, Ordering::Relaxed);
    c.total_ns.fetch_add(ns, Ordering::Relaxed);
    c.max_ns.fetch_max(ns, Ordering::Relaxed);
}

/// Record the time since `start`, if it was taken.
#[inline]
pub fn since(phase: Phase, start: Option<Instant>) {
    if let Some(start) = start {
        record(phase, start.elapsed());
    }
}

/// Times its scope into `phase` when profiling is on.
pub struct Span(Option<(Phase, Instant)>);

#[inline]
pub fn span(phase: Phase) -> Span {
    Span(now().map(|t| (phase, t)))
}

impl Drop for Span {
    #[inline]
    fn drop(&mut self) {
        if let Some((phase, start)) = self.0 {
            record(phase, start.elapsed());
        }
    }
}

pub fn reset() {
    for c in &COUNTERS {
        c.count.store(0, Ordering::Relaxed);
        c.total_ns.store(0, Ordering::Relaxed);
        c.max_ns.store(0, Ordering::Relaxed);
    }
    *SINCE.lock().unwrap() = Some(Instant::now());
}

/// A table of every phase that ran since the last reset.
pub fn report() -> String {
    let wall = SINCE.lock().unwrap().map(|t| t.elapsed()).unwrap_or_default();
    let mut out = format!(
        "phase breakdown over {:.1} s (times summed over threads)\n{:<22}{:>11}{:>12}{:>12}{:>12}\n",
        wall.as_secs_f64(),
        "phase",
        "count",
        "mean µs",
        "max µs",
        "total ms"
    );
    for phase in PHASES {
        let c = &COUNTERS[phase as usize];
        let count = c.count.load(Ordering::Relaxed);
        if count == 0 {
            continue;
        }
        let total = c.total_ns.load(Ordering::Relaxed);
        out.push_str(&format!(
            "{:<22}{:>11}{:>12.2}{:>12.1}{:>12.1}\n",
            phase.label(),
            count,
            total as f64 / count as f64 / 1e3,
            c.max_ns.load(Ordering::Relaxed) as f64 / 1e3,
            total as f64 / 1e6,
        ));
    }
    out
}
