//! Read-your-writes overlay.
//!
//! A reducer's writes are buffered in its call frame and only committed when
//! the reducer returns. The [`TableOverlay`] records, per table, the *pending*
//! state of every primary key the reducer touched so that
//!
//! * reads (`get`, `scan`, index scans) inside the same reducer see the
//!   reducer's own inserts / updates / deletes / clears, and
//! * write validation (primary key + unique indexes) is done against the
//!   merged view (committed rows + pending writes), not committed state only.
//!
//! The same structure is reused at commit time to re-validate a batch against
//! the committed table when another reducer committed to it in between, which
//! makes the commit all-or-nothing (see `Runtime::apply_all_transactions`).

use super::Table;
use crate::IntersticeError;
use interstice_abi::{IndexKey, IndexQuery, Row};
use wgpu::naga::FastHashMap;

#[derive(Debug)]
pub struct TableOverlay {
    /// `Table::version()` when the overlay was created. If the committed
    /// table still has this version at commit time, every validation done
    /// during the call is still valid and the commit can skip re-validation.
    pub base_version: u64,
    /// The reducer cleared the table: committed rows are invisible.
    cleared: bool,
    /// Pending state per primary key (`None` = deleted).
    rows: FastHashMap<IndexKey, Option<Row>>,
    /// Primary keys in first-touch order (stable scan order for new rows).
    order: Vec<IndexKey>,
    /// Unique-index keys held by live overlay rows: (index slot, key) -> pk.
    unique: FastHashMap<(usize, IndexKey), IndexKey>,
}

impl TableOverlay {
    pub fn new(base_version: u64) -> Self {
        Self {
            base_version,
            cleared: false,
            rows: FastHashMap::default(),
            order: Vec::new(),
            unique: FastHashMap::default(),
        }
    }

    #[inline]
    fn is_empty(&self) -> bool {
        !self.cleared && self.rows.is_empty()
    }
}

fn pk_of(row: &Row) -> Result<IndexKey, IntersticeError> {
    IndexKey::try_from(&row.primary_key).map_err(IntersticeError::Internal)
}

fn query_matches(query: &IndexQuery, key: &IndexKey) -> bool {
    match query {
        IndexQuery::Eq(k) => key == k,
        IndexQuery::Lt(k) => key < k,
        IndexQuery::Lte(k) => key <= k,
        IndexQuery::Gt(k) => key > k,
        IndexQuery::Gte(k) => key >= k,
        IndexQuery::Range {
            min,
            max,
            include_min,
            include_max,
        } => {
            let lo = if *include_min { key >= min } else { key > min };
            let hi = if *include_max { key <= max } else { key < max };
            lo && hi
        }
    }
}

impl Table {
    /// Monotonic counter bumped by every committed mutation.
    #[inline]
    pub fn version(&self) -> u64 {
        self.version
    }

    // ── Merged reads ────────────────────────────────────────────────────────

    /// Row visible under `ov` for `pk` (pending state wins over committed).
    pub fn ov_get<'a>(&'a self, ov: Option<&'a TableOverlay>, pk: &IndexKey) -> Option<&'a Row> {
        if let Some(ov) = ov {
            if let Some(pending) = ov.rows.get(pk) {
                return pending.as_ref();
            }
            if ov.cleared {
                return None;
            }
        }
        self.get_by_primary_key(pk)
    }

    #[inline]
    fn ov_exists(&self, ov: Option<&TableOverlay>, pk: &IndexKey) -> bool {
        self.ov_get(ov, pk).is_some()
    }

    pub fn ov_scan(&self, ov: Option<&TableOverlay>) -> Result<Vec<Row>, IntersticeError> {
        let Some(ov) = ov.filter(|o| !o.is_empty()) else {
            return Ok(self.rows.clone());
        };
        let mut out = Vec::with_capacity(self.rows.len() + ov.rows.len());
        if !ov.cleared {
            for row in &self.rows {
                match ov.rows.get(&pk_of(row)?) {
                    None => out.push(row.clone()),
                    Some(Some(pending)) => out.push(pending.clone()),
                    Some(None) => {}
                }
            }
        }
        // Rows that only exist in the overlay, in first-touch order.
        for pk in &ov.order {
            if !ov.cleared && self.primary_key_index.contains_key(pk) {
                continue; // already emitted in place above
            }
            if let Some(Some(row)) = ov.rows.get(pk) {
                out.push(row.clone());
            }
        }
        Ok(out)
    }

    pub fn ov_index_scan(
        &self,
        ov: Option<&TableOverlay>,
        field_name: &str,
        query: &IndexQuery,
    ) -> Result<Vec<Row>, IntersticeError> {
        // Committed lookup first: also surfaces IndexNotFound / unsupported query errors.
        let committed = self.get_by_index(field_name, query)?;
        let Some(ov) = ov.filter(|o| !o.is_empty()) else {
            return Ok(committed.into_iter().cloned().collect());
        };
        let table_index = self
            .indexes
            .iter()
            .find(|i| i.field_name == field_name)
            .expect("index existence checked by get_by_index");

        let mut out: Vec<(IndexKey, Row)> = Vec::new();
        if !ov.cleared {
            for row in committed {
                if ov.rows.contains_key(&pk_of(row)?) {
                    continue; // overridden: considered below with its pending value
                }
                out.push((table_index.key_from_row(row)?, row.clone()));
            }
        }
        for pk in &ov.order {
            if let Some(Some(row)) = ov.rows.get(pk) {
                let key = table_index.key_from_row(row)?;
                if query_matches(query, &key) {
                    out.push((key, row.clone()));
                }
            }
        }
        if matches!(table_index.index, super::auto_inc::IndexImpl::BTree(_)) {
            out.sort_by(|a, b| a.0.cmp(&b.0));
        }
        Ok(out.into_iter().map(|(_, r)| r).collect())
    }

    // ── Merged validation ───────────────────────────────────────────────────

    /// Is unique key `key` of index slot `slot` held by a visible row other than `self_pk`?
    fn ov_unique_taken(
        &self,
        ov: Option<&TableOverlay>,
        slot: usize,
        key: &IndexKey,
        self_pk: &IndexKey,
    ) -> Result<bool, IntersticeError> {
        if let Some(ov) = ov {
            if let Some(holder) = ov.unique.get(&(slot, key.clone())) {
                if holder != self_pk {
                    return Ok(true);
                }
            }
            if ov.cleared {
                return Ok(false);
            }
        }
        if let Some(positions) = self.indexes[slot].positions(key) {
            for &pos in positions {
                let pk = pk_of(&self.rows[pos])?;
                if &pk == self_pk {
                    continue;
                }
                if ov.is_some_and(|o| o.rows.contains_key(&pk)) {
                    continue; // committed version overridden by the overlay
                }
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn unique_violation(&self, slot: usize) -> IntersticeError {
        IntersticeError::UniqueConstraintViolation {
            table_name: self.schema.name.clone(),
            field_name: self.indexes[slot].field_name.clone(),
        }
    }

    pub fn ov_validate_insert(&self, ov: Option<&TableOverlay>, row: &Row) -> Result<(), IntersticeError> {
        let pk = pk_of(row)?;
        if self.ov_exists(ov, &pk) {
            return Err(IntersticeError::UniqueConstraintViolation {
                table_name: self.schema.name.clone(),
                field_name: self.schema.primary_key.name.clone(),
            });
        }
        for slot in 0..self.indexes.len() {
            if self.indexes[slot].unique {
                let key = self.indexes[slot].key_from_row(row)?;
                if self.ov_unique_taken(ov, slot, &key, &pk)? {
                    return Err(self.unique_violation(slot));
                }
            }
        }
        Ok(())
    }

    pub fn ov_validate_update(&self, ov: Option<&TableOverlay>, row: &Row) -> Result<(), IntersticeError> {
        let pk = pk_of(row)?;
        let existing = self
            .ov_get(ov, &pk)
            .ok_or_else(|| IntersticeError::RowNotFound {
                primary_key_value: pk.clone(),
            })?;
        for slot in 0..self.indexes.len() {
            let index = &self.indexes[slot];
            let old_key = index.key_from_row(existing)?;
            let new_key = index.key_from_row(row)?;
            if old_key == new_key {
                continue;
            }
            if index.auto_inc {
                return Err(IntersticeError::AutoIncUpdateNotAllowed {
                    table_name: self.schema.name.clone(),
                    field_name: index.field_name.clone(),
                });
            }
            if index.unique && self.ov_unique_taken(ov, slot, &new_key, &pk)? {
                return Err(self.unique_violation(slot));
            }
        }
        Ok(())
    }

    pub fn ov_validate_delete(&self, ov: Option<&TableOverlay>, pk: &IndexKey) -> Result<(), IntersticeError> {
        if self.ov_exists(ov, pk) {
            Ok(())
        } else {
            Err(IntersticeError::RowNotFound {
                primary_key_value: pk.clone(),
            })
        }
    }

    // ── Overlay recording (call after a successful validation) ─────────────

    /// Set the pending state of `pk` to `row` (`None` = deleted).
    pub fn ov_record(&self, ov: &mut TableOverlay, pk: IndexKey, row: Option<Row>) -> Result<(), IntersticeError> {
        // Drop unique keys held by the previous pending version.
        if let Some(Some(prev)) = ov.rows.get(&pk) {
            for (slot, index) in self.indexes.iter().enumerate() {
                if index.unique {
                    let key = index.key_from_row(prev)?;
                    if ov.unique.get(&(slot, key.clone())) == Some(&pk) {
                        ov.unique.remove(&(slot, key));
                    }
                }
            }
        }
        if let Some(new) = &row {
            for (slot, index) in self.indexes.iter().enumerate() {
                if index.unique {
                    ov.unique.insert((slot, index.key_from_row(new)?), pk.clone());
                }
            }
        }
        if ov.rows.insert(pk.clone(), row).is_none() {
            ov.order.push(pk);
        }
        Ok(())
    }

    pub fn ov_record_clear(&self, ov: &mut TableOverlay) {
        ov.cleared = true;
        ov.rows.clear();
        ov.order.clear();
        ov.unique.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use interstice_abi::{
        FieldDef, IndexSchema, IndexType, IntersticeType, IntersticeValue, PersistenceKind,
        TableSchema, TableVisibility,
    };

    /// Table `t(id: u64 pk, name: string unique hash, rank: u64 btree)`.
    fn table() -> Table {
        let id = FieldDef { name: "id".into(), field_type: IntersticeType::U64 };
        let name = FieldDef { name: "name".into(), field_type: IntersticeType::String };
        let rank = FieldDef { name: "rank".into(), field_type: IntersticeType::U64 };
        Table::new(TableSchema {
            name: "t".into(),
            type_name: "T".into(),
            visibility: TableVisibility::Public,
            fields: vec![name, rank],
            primary_key: id,
            primary_key_auto_inc: false,
            indexes: vec![
                IndexSchema { field_name: "name".into(), index_type: IndexType::Hash, unique: true, auto_inc: false },
                IndexSchema { field_name: "rank".into(), index_type: IndexType::BTree, unique: false, auto_inc: false },
            ],
            persistence: PersistenceKind::Ephemeral,
            retention: None,
        })
    }

    fn event_table(keep: u64) -> Table {
        let mut t = table();
        t.schema.retention = Some(keep);
        Table::new(t.schema)
    }

    fn sorted(mut v: Vec<u64>) -> Vec<u64> {
        v.sort();
        v
    }

    #[test]
    fn event_table_keeps_newest_rows() {
        let mut t = event_table(3);
        for i in 1..=5 {
            t.insert(row(i, &format!("r{i}"), i)).unwrap();
        }
        // Eviction only happens when the commit is done.
        assert_eq!(t.scan().len(), 5);
        assert_eq!(t.enforce_retention(), 2);
        assert_eq!(sorted(ids(t.scan())), vec![3, 4, 5]);
        // Evicted rows are gone from the indexes too.
        assert!(t.get_by_primary_key(&pk(1)).is_none());
        assert!(t.get_by_index("name", &IndexQuery::Eq(IndexKey::String("r2".into()))).unwrap().is_empty());
        // The unique index slot of an evicted row is free again.
        t.insert(row(6, "r1", 6)).unwrap();
        t.enforce_retention();
        assert_eq!(sorted(ids(t.scan())), vec![4, 5, 6]);
    }

    #[test]
    fn event_table_eviction_skips_deleted_rows() {
        let mut t = event_table(2);
        t.insert(row(1, "a", 1)).unwrap();
        t.insert(row(2, "b", 2)).unwrap();
        t.delete(&pk(1)).unwrap();
        // Re-inserting a deleted key makes it the newest, not the oldest.
        t.insert(row(1, "a", 1)).unwrap();
        t.insert(row(3, "c", 3)).unwrap();
        t.enforce_retention();
        assert_eq!(sorted(ids(t.scan())), vec![1, 3]);
        t.clear();
        t.insert(row(4, "d", 4)).unwrap();
        assert_eq!(t.enforce_retention(), 0);
        assert_eq!(ids(t.scan()), vec![4]);
    }

    fn row(id: u64, name: &str, rank: u64) -> Row {
        Row {
            primary_key: IntersticeValue::U64(id),
            entries: vec![IntersticeValue::String(name.into()), IntersticeValue::U64(rank)],
        }
    }

    fn pk(id: u64) -> IndexKey {
        IndexKey::U64(id)
    }

    fn ids(rows: &[Row]) -> Vec<u64> {
        rows.iter()
            .map(|r| match r.primary_key {
                IntersticeValue::U64(v) => v,
                _ => unreachable!(),
            })
            .collect()
    }

    fn stage_insert(t: &Table, ov: &mut TableOverlay, r: Row) -> Result<(), IntersticeError> {
        t.ov_validate_insert(Some(ov), &r)?;
        t.ov_record(ov, pk_of(&r)?, Some(r))
    }

    #[test]
    fn reads_see_pending_writes() {
        let mut t = table();
        t.insert(row(1, "a", 10)).unwrap();
        t.insert(row(2, "b", 20)).unwrap();
        let mut ov = TableOverlay::new(t.version());

        stage_insert(&t, &mut ov, row(3, "c", 5)).unwrap();
        t.ov_validate_update(Some(&ov), &row(1, "a2", 30)).unwrap();
        t.ov_record(&mut ov, pk(1), Some(row(1, "a2", 30))).unwrap();
        t.ov_validate_delete(Some(&ov), &pk(2)).unwrap();
        t.ov_record(&mut ov, pk(2), None).unwrap();

        assert_eq!(ids(&t.ov_scan(Some(&ov)).unwrap()), vec![1, 3]);
        assert!(t.ov_get(Some(&ov), &pk(2)).is_none());
        assert_eq!(t.ov_get(Some(&ov), &pk(1)).unwrap().entries[1], IntersticeValue::U64(30));
        // BTree scan merges and stays key-ordered.
        let ranked = t.ov_index_scan(Some(&ov), "rank", &IndexQuery::Gte(IndexKey::U64(0))).unwrap();
        assert_eq!(ids(&ranked), vec![3, 1]);
        // Committed table untouched.
        assert_eq!(ids(t.scan()), vec![1, 2]);
    }

    #[test]
    fn validation_uses_merged_view() {
        let mut t = table();
        t.insert(row(1, "a", 0)).unwrap();
        let mut ov = TableOverlay::new(t.version());

        // Duplicate pk within the same call.
        stage_insert(&t, &mut ov, row(2, "b", 0)).unwrap();
        assert!(stage_insert(&t, &mut ov, row(2, "x", 0)).is_err());
        // Unique key held by a pending row.
        assert!(stage_insert(&t, &mut ov, row(3, "b", 0)).is_err());
        // Unique key freed by a pending update can be reused.
        t.ov_record(&mut ov, pk(1), Some(row(1, "a-renamed", 0))).unwrap();
        stage_insert(&t, &mut ov, row(4, "a", 0)).unwrap();
        // Delete then re-insert the same pk.
        t.ov_record(&mut ov, pk(4), None).unwrap();
        stage_insert(&t, &mut ov, row(4, "d", 0)).unwrap();
        // Update of a row inserted in the same call.
        t.ov_validate_update(Some(&ov), &row(2, "b2", 1)).unwrap();
        // After clear, committed rows are gone.
        t.ov_record_clear(&mut ov);
        assert!(t.ov_validate_delete(Some(&ov), &pk(1)).is_err());
        stage_insert(&t, &mut ov, row(1, "a", 0)).unwrap();
        assert_eq!(ids(&t.ov_scan(Some(&ov)).unwrap()), vec![1]);
    }

    #[test]
    fn version_tracks_mutations() {
        let mut t = table();
        let v0 = t.version();
        t.insert(row(1, "a", 0)).unwrap();
        assert_ne!(t.version(), v0);
    }
}
