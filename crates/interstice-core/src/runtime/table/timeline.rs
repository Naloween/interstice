//! Table timelines: a commit tree a module can travel (DESIGN §6.8).

use super::Table;
use interstice_abi::{IndexKey, Row, TimelineInfo, TimelineNode, TimelinePoint};
use std::collections::{BTreeMap, HashMap};

/// The id of the base node: the oldest reachable state. Transaction ids
/// start at 1, so no commit is ever `0`.
pub const BASE: u64 = 0;

/// One row change of a commit, with its before and after images.
#[derive(Clone, Debug)]
pub enum TimelineOp {
    Insert(Row),
    Update { old: Row, new: Row },
    Delete(Row),
}

#[derive(Clone, Debug)]
struct Node {
    parent: u64,
    timestamp_ms: u64,
    ops: Vec<TimelineOp>,
    /// In commit order.
    children: Vec<u64>,
}

#[derive(Clone, Debug)]
pub struct Timeline {
    capacity: usize,
    /// Every node, the base included.
    nodes: BTreeMap<u64, Node>,
    head: u64,
}

pub type RowMap = HashMap<IndexKey, Row>;

fn pk(row: &Row) -> Option<IndexKey> {
    IndexKey::try_from(&row.primary_key).ok()
}

impl Timeline {
    pub fn new(capacity: usize) -> Self {
        let mut nodes = BTreeMap::new();
        nodes.insert(
            BASE,
            Node { parent: BASE, timestamp_ms: 0, ops: Vec::new(), children: Vec::new() },
        );
        Self { capacity: capacity.max(1), nodes, head: BASE }
    }

    pub fn head(&self) -> u64 {
        self.head
    }

    pub fn contains(&self, id: u64) -> bool {
        self.nodes.contains_key(&id)
    }

    pub fn info(&self) -> TimelineInfo {
        TimelineInfo {
            head: self.head,
            nodes: self
                .nodes
                .iter()
                .filter(|(id, _)| **id != BASE)
                .map(|(id, n)| TimelineNode {
                    id: *id,
                    parent: n.parent,
                    timestamp_ms: n.timestamp_ms,
                    ops: n.ops.len() as u32,
                })
                .collect(),
        }
    }

    /// `id`, its parent, …, the base.
    fn path_to_base(&self, mut id: u64) -> Vec<u64> {
        let mut path = vec![id];
        while id != BASE {
            id = self.nodes[&id].parent;
            path.push(id);
        }
        path
    }

    /// Resolve a point against the head.
    pub fn resolve(&self, point: TimelinePoint) -> Result<u64, String> {
        match point {
            TimelinePoint::Base => Ok(BASE),
            TimelinePoint::Commit(id) if self.nodes.contains_key(&id) => Ok(id),
            TimelinePoint::Commit(id) => Err(format!("commit {id} is not in the timeline")),
            TimelinePoint::Back(n) => {
                let mut id = self.head;
                for _ in 0..n {
                    if id == BASE {
                        break;
                    }
                    id = self.nodes[&id].parent;
                }
                Ok(id)
            }
            TimelinePoint::Forward(n) => {
                let mut id = self.head;
                for _ in 0..n {
                    match self.nodes[&id].children.last() {
                        Some(&child) => id = child,
                        None => break,
                    }
                }
                Ok(id)
            }
            TimelinePoint::Time(ms) => Ok(self
                .path_to_base(self.head)
                .into_iter()
                .find(|id| *id != BASE && self.nodes[id].timestamp_ms <= ms)
                .unwrap_or(BASE)),
        }
    }

    /// The rows at `target`, given `current`, the rows at the head.
    pub fn state_at(&self, current: &[Row], target: u64) -> Result<RowMap, String> {
        if !self.nodes.contains_key(&target) {
            return Err(format!("commit {target} is not in the timeline"));
        }
        let mut rows: RowMap = current.iter().filter_map(|r| Some((pk(r)?, r.clone()))).collect();
        let up = self.path_to_base(self.head);
        let down = self.path_to_base(target);
        let common = *up.iter().find(|id| down.contains(id)).expect("both paths end at the base");
        for id in up.iter().take_while(|id| **id != common) {
            for op in self.nodes[id].ops.iter().rev() {
                match op {
                    TimelineOp::Insert(row) => {
                        if let Some(k) = pk(row) {
                            rows.remove(&k);
                        }
                    }
                    TimelineOp::Update { old, .. } | TimelineOp::Delete(old) => {
                        if let Some(k) = pk(old) {
                            rows.insert(k, old.clone());
                        }
                    }
                }
            }
        }
        let redo: Vec<u64> = down.into_iter().take_while(|id| *id != common).collect();
        for id in redo.iter().rev() {
            for op in &self.nodes[id].ops {
                match op {
                    TimelineOp::Insert(row) | TimelineOp::Update { new: row, .. } => {
                        if let Some(k) = pk(row) {
                            rows.insert(k, row.clone());
                        }
                    }
                    TimelineOp::Delete(old) => {
                        if let Some(k) = pk(old) {
                            rows.remove(&k);
                        }
                    }
                }
            }
        }
        Ok(rows)
    }

    /// Move the head (commit bookkeeping; the rows were already written).
    /// `truncate` drops the subtree, under the common ancestor, that holds
    /// the old head. Returns false if `to` is gone.
    fn travel(&mut self, to: u64, truncate: bool) -> bool {
        if !self.nodes.contains_key(&to) {
            return false;
        }
        if truncate {
            let up = self.path_to_base(self.head);
            let down = self.path_to_base(to);
            if let Some(pos) = up.iter().position(|id| down.contains(id)) {
                // `up[pos]` is the common ancestor; `up[pos - 1]` its child
                // towards the old head.
                if pos > 0 {
                    self.remove_subtree(up[pos - 1]);
                }
            }
        }
        self.head = to;
        true
    }

    /// Add a commit as a child of the head and move the head to it.
    fn push(&mut self, id: u64, timestamp_ms: u64, ops: Vec<TimelineOp>) {
        let parent = self.head;
        self.nodes.get_mut(&parent).unwrap().children.push(id);
        self.nodes.insert(id, Node { parent, timestamp_ms, ops, children: Vec::new() });
        self.head = id;
        self.enforce_capacity();
    }

    fn remove_subtree(&mut self, root: u64) {
        if root == BASE {
            return;
        }
        if let Some(parent) = self.nodes.get(&root).map(|n| n.parent) {
            if let Some(p) = self.nodes.get_mut(&parent) {
                p.children.retain(|c| *c != root);
            }
        }
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            if let Some(node) = self.nodes.remove(&id) {
                stack.extend(node.children);
            }
        }
    }

    /// Fold the base forward until at most `capacity` commits remain.
    fn enforce_capacity(&mut self) {
        while self.nodes.len() - 1 > self.capacity {
            if self.head != BASE {
                let path = self.path_to_base(self.head);
                let keep = path[path.len() - 2];
                let others: Vec<u64> = self.nodes[&BASE]
                    .children
                    .iter()
                    .copied()
                    .filter(|c| *c != keep)
                    .collect();
                for other in others {
                    self.remove_subtree(other);
                }
                if self.nodes.len() - 1 <= self.capacity {
                    break;
                }
                // Merge `keep` into the base.
                let folded = self.nodes.remove(&keep).unwrap();
                for child in &folded.children {
                    self.nodes.get_mut(child).unwrap().parent = BASE;
                }
                self.nodes.get_mut(&BASE).unwrap().children = folded.children;
            } else {
                let Some(&oldest) = self.nodes[&BASE].children.first() else { break };
                self.remove_subtree(oldest);
            }
        }
    }
}

/// Rows of a [`RowMap`] in a deterministic order (encoded primary key).
pub fn sorted_rows(rows: RowMap) -> Vec<(IndexKey, Row)> {
    let mut rows: Vec<(Vec<u8>, IndexKey, Row)> = rows
        .into_iter()
        .map(|(k, r)| (interstice_abi::encode(&k).unwrap_or_default(), k, r))
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows.into_iter().map(|(_, k, r)| (k, r)).collect()
}

/// The timeline side of one commit, shared by live commits and
/// `record::apply_writes` so they evolve the tree identically.
#[derive(Default)]
pub(crate) struct TimelineCommit {
    tables: HashMap<String, Pending>,
}

#[derive(Default)]
struct Pending {
    travel: Option<(u64, bool)>,
    ops: Vec<TimelineOp>,
}

impl TimelineCommit {
    pub fn op(&mut self, table: &str, op: TimelineOp) {
        self.tables.entry(table.to_string()).or_default().ops.push(op);
    }

    /// A travel marker: the ops before it netted out to the target state.
    pub fn travel(&mut self, table: &str, to: u64, truncate: bool) {
        let pending = self.tables.entry(table.to_string()).or_default();
        pending.ops.clear();
        pending.travel = Some((to, truncate));
    }

    pub fn finish(self, tables: &mut HashMap<String, Table>, tx_id: u64, timestamp_ms: u64) {
        for (name, pending) in self.tables {
            let Some(timeline) = tables.get_mut(&name).and_then(Table::timeline_mut) else {
                continue;
            };
            if let Some((to, truncate)) = pending.travel {
                timeline.travel(to, truncate);
            }
            if !pending.ops.is_empty() {
                timeline.push(tx_id, timestamp_ms, pending.ops);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use interstice_abi::IntersticeValue;

    fn row(id: u64, v: u64) -> Row {
        Row { primary_key: IntersticeValue::U64(id), entries: vec![IntersticeValue::U64(v)] }
    }

    fn value(rows: &RowMap, id: u64) -> Option<u64> {
        rows.get(&IndexKey::try_from(&IntersticeValue::U64(id)).unwrap()).map(|r| match r.entries[0] {
            IntersticeValue::U64(v) => v,
            _ => unreachable!(),
        })
    }

    #[test]
    fn undo_redo_and_branch() {
        let mut t = Timeline::new(16);
        // 1: insert (1, 10); 2: update to 20; current rows: (1, 20).
        t.push(1, 100, vec![TimelineOp::Insert(row(1, 10))]);
        t.push(2, 200, vec![TimelineOp::Update { old: row(1, 10), new: row(1, 20) }]);
        let current = vec![row(1, 20)];
        assert_eq!(value(&t.state_at(&current, 1).unwrap(), 1), Some(10));
        assert_eq!(value(&t.state_at(&current, BASE).unwrap(), 1), None);
        assert_eq!(t.resolve(TimelinePoint::Back(1)).unwrap(), 1);
        assert_eq!(t.resolve(TimelinePoint::Back(9)).unwrap(), BASE);
        assert_eq!(t.resolve(TimelinePoint::Time(150)).unwrap(), 1);

        // Travel back to 1, then branch with commit 3.
        assert!(t.travel(1, false));
        t.push(3, 300, vec![TimelineOp::Update { old: row(1, 10), new: row(1, 30) }]);
        let current = vec![row(1, 30)];
        assert_eq!(value(&t.state_at(&current, 2).unwrap(), 1), Some(20));
        assert_eq!(t.info().nodes.len(), 3);
        // Forward from 1 picks the newest child.
        assert!(t.travel(1, false));
        assert_eq!(t.resolve(TimelinePoint::Forward(1)).unwrap(), 3);

        // Truncate: from 3 back to 1 drops 3 only.
        assert!(t.travel(3, false));
        assert!(t.travel(1, true));
        assert!(!t.contains(3) && t.contains(2));
    }

    #[test]
    fn capacity_folds_the_base() {
        let mut t = Timeline::new(2);
        t.push(1, 1, vec![TimelineOp::Insert(row(1, 1))]);
        t.push(2, 2, vec![TimelineOp::Insert(row(2, 2))]);
        t.travel(1, false);
        t.push(3, 3, vec![TimelineOp::Insert(row(3, 3))]);
        // 3 commits > 2: 1, the oldest on the head's path, folds into the
        // base; branch 2 becomes a child of the base.
        assert!(!t.contains(1) && t.contains(2) && t.contains(3));
        t.push(4, 4, vec![TimelineOp::Insert(row(4, 4))]);
        // Then the branch off the head's path goes first.
        assert!(!t.contains(2));
        assert_eq!(t.info().nodes.iter().map(|n| (n.id, n.parent)).collect::<Vec<_>>(), vec![(3, BASE), (4, 3)]);
        let current = vec![row(1, 1), row(3, 3), row(4, 4)];
        let base = t.state_at(&current, BASE).unwrap();
        assert_eq!(base.len(), 1);
        assert_eq!(value(&base, 1), Some(1));
    }
}
