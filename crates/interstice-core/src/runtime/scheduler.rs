use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableOp {
    Read,
    Insert,
    Update,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableAccess {
    pub table_name: String,
    pub op: TableOp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledJob<T> {
    pub id: u64,
    pub payload: T,
    pub accesses: Vec<TableAccess>,
}

#[derive(Debug)]
pub struct ReducerScheduler<T> {
    next_id: u64,
    running: Vec<(u64, Vec<TableAccess>)>,
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

    pub fn enqueue(&mut self, payload: T, accesses: Vec<TableAccess>) -> Option<ScheduledJob<T>> {
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
            .any(|(_, accesses)| conflicts(&job.accesses, accesses))
        {
            return false;
        }
        for waiting in &self.waiting {
            if conflicts(&job.accesses, &waiting.accesses) {
                return false;
            }
        }
        true
    }

    fn promote_waiting(&mut self) -> Vec<ScheduledJob<T>> {
        let mut promoted = Vec::new();
        let mut new_waiting = VecDeque::new();
        let mut blockers: Vec<Vec<TableAccess>> = Vec::new();

        while let Some(job) = self.waiting.pop_front() {
            let blocked_by_running = self
                .running
                .iter()
                .any(|(_, accesses)| conflicts(&job.accesses, accesses));
            let blocked_by_earlier_waiting =
                blockers.iter().any(|access| conflicts(&job.accesses, access));
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

fn conflicts(a: &[TableAccess], b: &[TableAccess]) -> bool {
    for left in a {
        for right in b {
            if left.table_name != right.table_name {
                continue;
            }
            if op_conflicts(left.op, right.op) {
                return true;
            }
        }
    }
    false
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
    use super::{ReducerScheduler, TableAccess, TableOp};

    fn acc(table: &str, op: TableOp) -> TableAccess {
        TableAccess {
            table_name: table.to_string(),
            op,
        }
    }

    #[test]
    fn insert_insert_same_table_can_run_together() {
        let mut s = ReducerScheduler::new();
        let first = s.enqueue("a", vec![acc("t", TableOp::Insert)]);
        let first = first.into_iter().collect::<Vec<_>>();
        assert_eq!(first.len(), 1);
        let second = s.enqueue("b", vec![acc("t", TableOp::Insert)]);
        let second = second.into_iter().collect::<Vec<_>>();
        assert_eq!(second.len(), 1);
    }

    #[test]
    fn waiting_queue_allows_later_non_conflicting_jobs() {
        let mut s = ReducerScheduler::new();
        let running = s
            .enqueue("run", vec![acc("a", TableOp::Update)])
            .into_iter()
            .collect::<Vec<_>>();
        assert_eq!(running.len(), 1);

        // Both wait behind `run`'s update on `a`; reads do not conflict with each other once `run` completes.
        s.enqueue("wait1", vec![acc("a", TableOp::Read)]);
        s.enqueue("wait2", vec![acc("a", TableOp::Read)]);

        let promoted = s.complete(running[0].id);
        assert_eq!(promoted.len(), 2);
        assert_eq!(promoted[0].payload, "wait1");
        assert_eq!(promoted[1].payload, "wait2");
    }

    #[test]
    fn read_conflicts_with_insert_same_table() {
        let mut s = ReducerScheduler::new();
        let first = s
            .enqueue("reader", vec![acc("t", TableOp::Read)])
            .into_iter()
            .collect::<Vec<_>>();
        assert_eq!(first.len(), 1);

        s.enqueue("writer", vec![acc("t", TableOp::Insert)]);

        let promoted = s.complete(first[0].id);
        assert_eq!(promoted.len(), 1);
        assert_eq!(promoted[0].payload, "writer");
    }
}
