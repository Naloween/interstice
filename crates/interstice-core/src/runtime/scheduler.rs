use interstice_abi::ReducerTableRef;
use std::cmp::Ordering;
use std::collections::VecDeque;
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
}

#[derive(Debug)]
pub struct ReducerScheduler<T> {
    next_id: u64,
    running: Vec<(u64, Arc<[TableAccess]>)>,
    waiting: VecDeque<ScheduledJob<T>>,
}

impl<T> ReducerScheduler<T> {
    pub fn new() -> Self {
        Self {
            next_id: 0,
            running: Vec::new(),
            waiting: VecDeque::new(),
        }
    }

    pub fn enqueue(&mut self, payload: T, accesses: Arc<[TableAccess]>) -> Option<ScheduledJob<T>> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let job = ScheduledJob {
            id,
            payload,
            accesses,
        };
        if self.can_run_now(&job) {
            self.running.push((job.id, job.accesses.clone()));
            Some(job)
        } else {
            self.waiting.push_back(job);
            None
        }
    }

    pub fn complete(&mut self, completed_id: u64) -> Vec<ScheduledJob<T>> {
        if let Some(idx) = self.running.iter().position(|j| j.0 == completed_id) {
            self.running.remove(idx);
        }
        self.promote_waiting()
    }

    fn can_run_now(&self, job: &ScheduledJob<T>) -> bool {
        if self
            .running
            .iter()
            .any(|(_, accesses)| conflicts_sorted(job.accesses.as_ref(), accesses.as_ref()))
        {
            return false;
        }
        for waiting in &self.waiting {
            if conflicts_sorted(job.accesses.as_ref(), waiting.accesses.as_ref()) {
                return false;
            }
        }
        true
    }

    fn promote_waiting(&mut self) -> Vec<ScheduledJob<T>> {
        let mut promoted = Vec::new();
        let mut new_waiting = VecDeque::new();
        let mut blockers: Vec<Arc<[TableAccess]>> = Vec::new();

        while let Some(job) = self.waiting.pop_front() {
            let blocked_by_running = self.running.iter().any(|(_, accesses)| {
                conflicts_sorted(job.accesses.as_ref(), accesses.as_ref())
            });
            let blocked_by_earlier_waiting = blockers.iter().any(|access| {
                conflicts_sorted(job.accesses.as_ref(), access.as_ref())
            });
            if blocked_by_running || blocked_by_earlier_waiting {
                blockers.push(job.accesses.clone());
                new_waiting.push_back(job);
            } else {
                self.running.push((job.id, job.accesses.clone()));
                promoted.push(job);
            }
        }

        self.waiting = new_waiting;
        promoted
    }
}

/// `a` and `b` must be sorted by `(key, op)` (see [`sort_table_accesses`]).
fn conflicts_sorted(a: &[TableAccess], b: &[TableAccess]) -> bool {
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

fn scan_same_table(slice: &[TableAccess], start: usize, tr: &AccessKey) -> usize {
    let mut k = start;
    while k < slice.len() && slice[k].key == *tr {
        k += 1;
    }
    k
}

pub(crate) fn sort_table_accesses(accesses: &mut [TableAccess]) {
    accesses.sort_unstable_by(|x, y| {
        x.key
            .cmp(&y.key)
            .then_with(|| x.op.cmp(&y.op))
    });
}

fn op_conflicts(a: TableOp, b: TableOp) -> bool {
    use TableOp::*;
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
}
