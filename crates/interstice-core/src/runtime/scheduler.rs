use interstice_abi::ReducerTableRef;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TableOp {
    Read,
    Insert,
    Update,
    Delete,
}

/// What a job touches. Ordering is only used to sort access sets for the
/// merge-style conflict check.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum AccessKey {
    Table(ReducerTableRef),
    /// Deferred calls from module `from` to module `to` (DESIGN §6.6). Taken
    /// with [`TableOp::Update`] so batches on one channel serialise in arrival
    /// order and conflict with nothing else.
    Channel { from: String, to: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableAccess {
    pub key: AccessKey,
    pub op: TableOp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledJob<T> {
    pub id: u64,
    pub payload: T,
    pub accesses: Arc<[TableAccess]>,
    /// When it was enqueued, if profiling (DESIGN §6.8).
    pub born: Option<std::time::Instant>,
}

/// The operations one job performs on one key, as a bit set.
type OpSet = u8;

fn op_bit(op: TableOp) -> OpSet {
    match op {
        TableOp::Read => 1,
        TableOp::Insert => 2,
        TableOp::Update => 4,
        TableOp::Delete => 8,
    }
}

/// Only `{Read}`/`{Read}` and `{Insert}`/`{Insert}` may run together (§6.2).
fn sets_compatible(a: OpSet, b: OpSet) -> bool {
    a == b && (a == 1 || a == 2)
}

/// One key's jobs. The clear ones run (or may run) together: they are
/// mutually compatible, so only their common op set matters. The others wait
/// in arrival order; the first of them conflicts with the clear set.
#[derive(Debug, Default)]
struct KeyQueue {
    clear: HashSet<u64>,
    clear_ops: OpSet,
    pending: VecDeque<(u64, OpSet)>,
}

#[derive(Debug)]
struct Waiting<T> {
    job: ScheduledJob<T>,
    /// Keys on which the job is not yet clear.
    blocked: usize,
}

/// Admits jobs by their access sets with one FIFO queue per key (DESIGN §6.2).
/// Conflicting jobs run in arrival order; others run in parallel. Enqueue is
/// O(keys), and each entry is promoted once, so completion is amortized
/// O(keys) too.
#[derive(Debug)]
pub struct ReducerScheduler<T> {
    next_id: u64,
    queues: BTreeMap<AccessKey, KeyQueue>,
    /// Keys and op sets of every running or waiting job.
    keys: HashMap<u64, Vec<(AccessKey, OpSet)>>,
    waiting: HashMap<u64, Waiting<T>>,
}

impl<T> Default for ReducerScheduler<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> ReducerScheduler<T> {
    pub fn new() -> Self {
        Self {
            next_id: 0,
            queues: BTreeMap::new(),
            keys: HashMap::new(),
            waiting: HashMap::new(),
        }
    }

    /// Jobs that are waiting.
    #[cfg(test)]
    pub fn waiting_len(&self) -> usize {
        self.waiting.len()
    }

    /// Jobs that are running.
    #[cfg(test)]
    pub fn running_len(&self) -> usize {
        self.keys.len() - self.waiting.len()
    }

    pub fn enqueue(&mut self, payload: T, accesses: Arc<[TableAccess]>) -> Option<ScheduledJob<T>> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);

        let mut sets: Vec<(AccessKey, OpSet)> = Vec::new();
        for access in accesses.iter() {
            match sets.iter_mut().find(|(key, _)| *key == access.key) {
                Some((_, ops)) => *ops |= op_bit(access.op),
                None => sets.push((access.key.clone(), op_bit(access.op))),
            }
        }

        let mut blocked = 0;
        for (key, ops) in &sets {
            let queue = self.queues.entry(key.clone()).or_default();
            let clear = queue.pending.is_empty()
                && (queue.clear.is_empty() || sets_compatible(queue.clear_ops, *ops));
            if clear {
                queue.clear.insert(id);
                queue.clear_ops = *ops;
            } else {
                blocked += 1;
                queue.pending.push_back((id, *ops));
            }
        }
        self.keys.insert(id, sets);

        let job = ScheduledJob { id, payload, accesses, born: crate::runtime::profile::now() };
        if blocked == 0 {
            Some(job)
        } else {
            self.waiting.insert(id, Waiting { job, blocked });
            None
        }
    }

    /// Mark a running job done; returns the jobs it releases, in arrival order.
    pub fn complete(&mut self, completed_id: u64) -> Vec<ScheduledJob<T>> {
        let Some(sets) = self.keys.remove(&completed_id) else {
            return Vec::new();
        };
        let mut released = Vec::new();
        for (key, _) in sets {
            let Some(queue) = self.queues.get_mut(&key) else { continue };
            queue.clear.remove(&completed_id);
            if !queue.clear.is_empty() {
                continue;
            }
            // The clear set is empty: the first waiting entry and every
            // compatible one after it become clear.
            if let Some(&(_, first)) = queue.pending.front() {
                queue.clear_ops = first;
                while let Some(&(job, ops)) = queue.pending.front() {
                    if !queue.clear.is_empty() && !sets_compatible(first, ops) {
                        break;
                    }
                    queue.pending.pop_front();
                    queue.clear.insert(job);
                    if let Some(w) = self.waiting.get_mut(&job) {
                        w.blocked -= 1;
                        if w.blocked == 0 {
                            released.push(job);
                        }
                    }
                }
            }
            if queue.clear.is_empty() && queue.pending.is_empty() {
                self.queues.remove(&key);
            }
        }
        released.sort_unstable();
        released
            .into_iter()
            .filter_map(|id| self.waiting.remove(&id).map(|w| w.job))
            .collect()
    }
}

pub(crate) fn sort_table_accesses(accesses: &mut [TableAccess]) {
    accesses.sort_unstable_by(|x, y| {
        x.key
            .cmp(&y.key)
            .then_with(|| x.op.cmp(&y.op))
    });
}

#[cfg(test)]
mod tests {
    use super::{AccessKey, ReducerScheduler, TableAccess, TableOp, sort_table_accesses};
    use interstice_abi::{ModuleSelection, NodeSelection, ReducerTableRef};
    use std::sync::Arc;

    fn acc(module: &str, table: &str, op: TableOp) -> TableAccess {
        TableAccess {
            key: AccessKey::Table(ReducerTableRef {
                node_selection: NodeSelection::Current,
                module_selection: ModuleSelection::Other(module.to_string()),
                table_name: table.to_string(),
            }),
            op,
        }
    }

    fn arc_accesses(mut v: Vec<TableAccess>) -> Arc<[TableAccess]> {
        sort_table_accesses(&mut v);
        Arc::from(v.into_boxed_slice())
    }

    #[test]
    fn insert_insert_same_table_can_run_together() {
        let mut s = ReducerScheduler::new();
        let first = s.enqueue("a", arc_accesses(vec![acc("m", "t", TableOp::Insert)]));
        let first = first.into_iter().collect::<Vec<_>>();
        assert_eq!(first.len(), 1);
        let second = s.enqueue("b", arc_accesses(vec![acc("m", "t", TableOp::Insert)]));
        let second = second.into_iter().collect::<Vec<_>>();
        assert_eq!(second.len(), 1);
    }

    #[test]
    fn waiting_queue_allows_later_non_conflicting_jobs() {
        let mut s = ReducerScheduler::new();
        let running = s
            .enqueue("run", arc_accesses(vec![acc("m", "a", TableOp::Update)]))
            .into_iter()
            .collect::<Vec<_>>();
        assert_eq!(running.len(), 1);

        // Both wait behind `run`'s update on `a`; reads do not conflict with each other once `run` completes.
        s.enqueue("wait1", arc_accesses(vec![acc("m", "a", TableOp::Read)]));
        s.enqueue("wait2", arc_accesses(vec![acc("m", "a", TableOp::Read)]));

        let promoted = s.complete(running[0].id);
        assert_eq!(promoted.len(), 2);
        assert_eq!(promoted[0].payload, "wait1");
        assert_eq!(promoted[1].payload, "wait2");
    }

    fn chan(from: &str, to: &str) -> TableAccess {
        TableAccess {
            key: AccessKey::Channel { from: from.into(), to: to.into() },
            op: TableOp::Update,
        }
    }

    #[test]
    fn same_channel_serialises_other_channels_do_not() {
        let mut s = ReducerScheduler::new();
        let first = s
            .enqueue("b1", arc_accesses(vec![chan("browser", "graphics"), acc("graphics", "d", TableOp::Insert)]))
            .into_iter()
            .collect::<Vec<_>>();
        assert_eq!(first.len(), 1);
        // Same channel waits even though Insert/Insert would not conflict.
        assert!(s
            .enqueue("b2", arc_accesses(vec![chan("browser", "graphics"), acc("graphics", "d", TableOp::Insert)]))
            .is_none());
        // Another caller to the same callee runs alongside.
        assert!(s
            .enqueue("a1", arc_accesses(vec![chan("agar", "graphics"), acc("graphics", "d", TableOp::Insert)]))
            .is_some());
        let promoted = s.complete(first[0].id);
        assert_eq!(promoted.len(), 1);
        assert_eq!(promoted[0].payload, "b2");
    }

    #[test]
    fn read_conflicts_with_insert_same_table() {
        let mut s = ReducerScheduler::new();
        let first = s
            .enqueue("reader", arc_accesses(vec![acc("m", "t", TableOp::Read)]))
            .into_iter()
            .collect::<Vec<_>>();
        assert_eq!(first.len(), 1);

        s.enqueue("writer", arc_accesses(vec![acc("m", "t", TableOp::Insert)]));

        let promoted = s.complete(first[0].id);
        assert_eq!(promoted.len(), 1);
        assert_eq!(promoted[0].payload, "writer");
    }

    // ---- Reference model: the previous O(n) scheduler, kept as the oracle.
    use super::{AccessKey as Key, TableAccess as Access, TableOp as Op};
    use std::cmp::Ordering;

/// `a` and `b` must be sorted by `(key, op)` (see [`sort_table_accesses`]).
pub(super) fn conflicts_sorted(a: &[Access], b: &[Access]) -> bool {
    let mut i = 0usize;
    let mut j = 0usize;
    while i < a.len() && j < b.len() {
        match a[i].key.cmp(&b[j].key) {
            Ordering::Less => i += 1,
            Ordering::Greater => j += 1,
            Ordering::Equal => {
                let tr = &a[i].key;
                let i1 = scan_same_table(a, i, tr);
                let j1 = scan_same_table(b, j, tr);
                for x in &a[i..i1] {
                    for y in &b[j..j1] {
                        if op_conflicts(x.op, y.op) {
                            return true;
                        }
                    }
                }
                i = i1;
                j = j1;
            }
        }
    }
    false
}

fn scan_same_table(slice: &[Access], start: usize, tr: &Key) -> usize {
    let mut k = start;
    while k < slice.len() && slice[k].key == *tr {
        k += 1;
    }
    k
}

fn op_conflicts(a: Op, b: Op) -> bool {
    use Op::*;
    match (a, b) {
        (Read, Read) => false,
        (Read, _) | (_, Read) => true,
        (Insert, Insert) => false,
        (Insert, Update) | (Update, Insert) => true,
        (Insert, Delete) | (Delete, Insert) => true,
        (Update, Update) => true,
        (Update, Delete) | (Delete, Update) => true,
        (Delete, Delete) => true,
    }
}


    struct Model {
        next: u64,
        running: Vec<(u64, Arc<[Access]>)>,
        waiting: Vec<(u64, Arc<[Access]>)>,
    }

    impl Model {
        fn enqueue(&mut self, a: Arc<[Access]>) -> Option<u64> {
            let id = self.next;
            self.next += 1;
            let free = !self.running.iter().chain(&self.waiting).any(|(_, b)| conflicts_sorted(&a, b));
            if free {
                self.running.push((id, a));
                Some(id)
            } else {
                self.waiting.push((id, a));
                None
            }
        }
        fn complete(&mut self, id: u64) -> Vec<u64> {
            self.running.retain(|(j, _)| *j != id);
            let mut out = Vec::new();
            let mut blockers: Vec<Arc<[Access]>> = Vec::new();
            let mut still = Vec::new();
            for (j, a) in std::mem::take(&mut self.waiting) {
                let blocked = self.running.iter().any(|(_, b)| conflicts_sorted(&a, b))
                    || blockers.iter().any(|b| conflicts_sorted(&a, b));
                if blocked {
                    blockers.push(a.clone());
                    still.push((j, a));
                } else {
                    self.running.push((j, a));
                    out.push(j);
                }
            }
            self.waiting = still;
            out
        }
    }

    #[test]
    fn matches_the_reference_model() {
        let ops = [Op::Read, Op::Insert, Op::Update, Op::Delete];
        let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut rand = move |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        for _round in 0..200 {
            let mut s = ReducerScheduler::new();
            let mut m = Model { next: 0, running: Vec::new(), waiting: Vec::new() };
            let mut running: Vec<u64> = Vec::new();
            for _step in 0..300 {
                if running.is_empty() || rand(3) != 0 {
                    let mut v = Vec::new();
                    for _ in 0..=rand(3) {
                        let t = ["a", "b", "c"][rand(3) as usize];
                        v.push(acc("m", t, ops[rand(4) as usize]));
                    }
                    let a = arc_accesses(v);
                    let got = s.enqueue((), a.clone()).map(|j| j.id);
                    assert_eq!(got, m.enqueue(a));
                    running.extend(got);
                } else {
                    let id = running.swap_remove(rand(running.len() as u64) as usize);
                    let got: Vec<u64> = s.complete(id).into_iter().map(|j| j.id).collect();
                    assert_eq!(got, m.complete(id));
                    running.extend(got);
                }
                assert_eq!(s.running_len(), m.running.len());
                assert_eq!(s.waiting_len(), m.waiting.len());
            }
        }
    }

    #[test]
    fn contention_is_linear() {
        // Conflicting jobs queued at once: the old scheduler was quadratic
        // (~18 s for 20 000). Compare sizes rather than wall time, so a busy
        // machine doesn't fail it: 4× the jobs must take well under 16×.
        fn run(n: u64) -> std::time::Duration {
            let mut s = ReducerScheduler::new();
            let t = std::time::Instant::now();
            let mut running = Vec::new();
            for i in 0..n {
                let a = arc_accesses(vec![acc("m", "run", TableOp::Read), acc("m", "t", TableOp::Update)]);
                running.extend(s.enqueue(i, a).map(|j| j.id));
            }
            let mut done = 0;
            while let Some(id) = running.pop() {
                done += 1;
                running.extend(s.complete(id).into_iter().map(|j| j.id));
            }
            assert_eq!(done, n);
            t.elapsed()
        }
        run(1_000); // warm up
        let small = (0..3).map(|_| run(5_000)).min().unwrap();
        let large = (0..3).map(|_| run(20_000)).min().unwrap();
        assert!(large < small * 8, "5 000 jobs: {small:?}, 20 000 jobs: {large:?}");
    }
}
