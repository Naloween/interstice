use crate::{IntersticeError, runtime::table::Table};
use interstice_abi::{IndexKey, IntersticeType, IntersticeValue, Row};
use std::collections::BTreeMap;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use wgpu::naga::FastHashMap;

/// The numeric type an auto-inc column was declared with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoIncKind {
    U8,
    U32,
    U64,
    I32,
    I64,
}

/// Lock-free auto-increment counter.
/// Cloning is cheap — the Arc inside means all clones share the same underlying counter,
/// so concurrent threads calling `next_value()` each receive a unique ID.
#[derive(Clone, Debug)]
pub(crate) struct AutoIncCounter {
    pub(crate) raw: Arc<AtomicU64>,
    pub(crate) kind: AutoIncKind,
}

impl AutoIncCounter {
    pub fn from_type(field_type: &IntersticeType) -> Option<Self> {
        let kind = match field_type {
            IntersticeType::U8 => AutoIncKind::U8,
            IntersticeType::U32 => AutoIncKind::U32,
            IntersticeType::U64 => AutoIncKind::U64,
            IntersticeType::I32 => AutoIncKind::I32,
            IntersticeType::I64 => AutoIncKind::I64,
            _ => return None,
        };
        Some(Self { raw: Arc::new(AtomicU64::new(0)), kind })
    }

    /// Atomically reserve the next ID.  Each call on any clone of the same counter
    /// returns a distinct value, making concurrent inserts collision-free.
    pub fn next_value(&self) -> Result<IntersticeValue, IntersticeError> {
        let id = self.raw.fetch_add(1, Ordering::Relaxed);
        match self.kind {
            AutoIncKind::U8 => {
                if id > u8::MAX as u64 {
                    return Err(IntersticeError::Internal("auto_inc overflow (U8)".into()));
                }
                Ok(IntersticeValue::U8(id as u8))
            }
            AutoIncKind::U32 => {
                if id > u32::MAX as u64 {
                    return Err(IntersticeError::Internal("auto_inc overflow (U32)".into()));
                }
                Ok(IntersticeValue::U32(id as u32))
            }
            AutoIncKind::U64 => Ok(IntersticeValue::U64(id)),
            AutoIncKind::I32 => {
                if id > i32::MAX as u64 {
                    return Err(IntersticeError::Internal("auto_inc overflow (I32)".into()));
                }
                Ok(IntersticeValue::I32(id as i32))
            }
            AutoIncKind::I64 => {
                if id > i64::MAX as u64 {
                    return Err(IntersticeError::Internal("auto_inc overflow (I64)".into()));
                }
                Ok(IntersticeValue::I64(id as i64))
            }
        }
    }

    /// Advance the counter so its next issued value is strictly greater than `value`.
    /// Used when loading persisted rows to restore the counter to the right position.
    pub fn sync_from_value(&self, value: &IntersticeValue) -> Result<(), IntersticeError> {
        let candidate: u64 = match (self.kind, value) {
            (AutoIncKind::U8, IntersticeValue::U8(v)) => (*v as u64)
                .checked_add(1)
                .ok_or_else(|| IntersticeError::Internal("auto_inc overflow".into()))?,
            (AutoIncKind::U32, IntersticeValue::U32(v)) => (*v as u64)
                .checked_add(1)
                .ok_or_else(|| IntersticeError::Internal("auto_inc overflow".into()))?,
            (AutoIncKind::U64, IntersticeValue::U64(v)) => v
                .checked_add(1)
                .ok_or_else(|| IntersticeError::Internal("auto_inc overflow".into()))?,
            (AutoIncKind::I32, IntersticeValue::I32(v)) => (*v as i64)
                .checked_add(1)
                .ok_or_else(|| IntersticeError::Internal("auto_inc overflow".into()))? as u64,
            (AutoIncKind::I64, IntersticeValue::I64(v)) => v
                .checked_add(1)
                .ok_or_else(|| IntersticeError::Internal("auto_inc overflow".into()))? as u64,
            _ => {
                return Err(IntersticeError::Internal(
                    "auto_inc value type mismatch".into(),
                ))
            }
        };
        // Advance atomically to max(current, candidate)
        let mut current = self.raw.load(Ordering::Relaxed);
        while candidate > current {
            match self.raw.compare_exchange_weak(
                current,
                candidate,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(updated) => current = updated,
            }
        }
        Ok(())
    }

    pub fn reset(&self) {
        self.raw.store(0, Ordering::Relaxed);
    }
}

/// A snapshot of a table's auto-inc counters for one reducer call.
/// Holds Arc clones of the table's live atomic counters — calling `next_value()` on
/// any snapshot (from any thread) atomically advances the shared counter, so no two
/// concurrent reducer calls will ever receive the same ID.
#[derive(Debug)]
pub(crate) struct TableAutoIncSnapshot {
    pub primary: Option<AutoIncCounter>,
    pub indexes: Vec<Option<AutoIncCounter>>,
}

pub enum IndexImpl {
    Hash(FastHashMap<IndexKey, Vec<usize>>),
    BTree(BTreeMap<IndexKey, Vec<usize>>),
}

impl Table {
    /// Return a snapshot of this table's auto-inc counters.
    /// Because the snapshot shares the same Arc<AtomicU64> as the table, concurrent
    /// threads all advance the *same* atomic and receive non-overlapping IDs.
    pub(crate) fn auto_inc_snapshot(&self) -> TableAutoIncSnapshot {
        TableAutoIncSnapshot {
            primary: self.primary_key_auto_inc_counter.clone(),
            indexes: self
                .indexes
                .iter()
                .map(|index| index.auto_inc_counter.clone())
                .collect(),
        }
    }

    pub(crate) fn apply_auto_inc_from_snapshot(
        &self,
        row: &mut Row,
        snapshot: &TableAutoIncSnapshot,
    ) -> Result<(), IntersticeError> {
        if self.primary_key_auto_inc {
            let counter = snapshot.primary.as_ref().ok_or_else(|| {
                IntersticeError::Internal(format!(
                    "auto_inc counter missing for primary key in table '{}'",
                    self.schema.name
                ))
            })?;
            row.primary_key = counter.next_value()?;
        }

        for (index, table_index) in self.indexes.iter().enumerate() {
            if !table_index.auto_inc {
                continue;
            }
            let counter = snapshot
                .indexes
                .get(index)
                .and_then(|s| s.as_ref())
                .ok_or_else(|| {
                    IntersticeError::Internal(format!(
                        "auto_inc counter missing for field '{}' in table '{}'",
                        table_index.field_name, self.schema.name
                    ))
                })?;
            let value = counter.next_value()?;
            if let Some(entry) = row.entries.get_mut(table_index.field_index) {
                *entry = value;
            } else {
                return Err(IntersticeError::Internal(
                    "Index field out of bounds".to_string(),
                ));
            }
        }

        Ok(())
    }

    pub(crate) fn sync_auto_inc_from_row(&self, row: &Row) -> Result<(), IntersticeError> {
        if self.primary_key_auto_inc {
            let counter = self.primary_key_auto_inc_counter.as_ref().ok_or_else(|| {
                IntersticeError::Internal(format!(
                    "auto_inc counter missing for primary key in table '{}'",
                    self.schema.name
                ))
            })?;
            counter.sync_from_value(&row.primary_key)?;
        }
        for table_index in &self.indexes {
            table_index.sync_auto_inc_from_row(row, &self.schema.name)?;
        }
        Ok(())
    }
}
