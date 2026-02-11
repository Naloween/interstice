use interstice_abi::{
    IndexKey, IntersticeType, IntersticeValue, Row
};
use std::collections::BTreeMap;
use wgpu::naga::FastHashMap;
use crate::{IntersticeError, runtime::table::Table};

#[derive(Clone, Debug)]
pub enum AutoIncState {
    U8(u8),
    U32(u32),
    U64(u64),
    I32(i32),
    I64(i64),
}

#[derive(Debug)]
pub(crate) struct TableAutoIncSnapshot {
    pub primary: Option<AutoIncState>,
    pub indexes: Vec<Option<AutoIncState>>,
}



pub enum IndexImpl {
    Hash(FastHashMap<IndexKey, Vec<usize>>),
    BTree(BTreeMap<IndexKey, Vec<usize>>),
}

impl AutoIncState {
    pub fn from_type(field_type: &IntersticeType) -> Option<Self> {
        match field_type {
            IntersticeType::U8 => Some(Self::U8(0)),
            IntersticeType::U32 => Some(Self::U32(0)),
            IntersticeType::U64 => Some(Self::U64(0)),
            IntersticeType::I32 => Some(Self::I32(0)),
            IntersticeType::I64 => Some(Self::I64(0)),
            _ => None,
        }
    }

    pub fn next_value(&mut self) -> Result<IntersticeValue, IntersticeError> {
        match self {
            AutoIncState::U8(value) => {
                let current = *value;
                *value = value
                    .checked_add(1)
                    .ok_or_else(|| IntersticeError::Internal("auto_inc overflow".into()))?;
                Ok(IntersticeValue::U8(current))
            }
            AutoIncState::U32(value) => {
                let current = *value;
                *value = value
                    .checked_add(1)
                    .ok_or_else(|| IntersticeError::Internal("auto_inc overflow".into()))?;
                Ok(IntersticeValue::U32(current))
            }
            AutoIncState::U64(value) => {
                let current = *value;
                *value = value
                    .checked_add(1)
                    .ok_or_else(|| IntersticeError::Internal("auto_inc overflow".into()))?;
                Ok(IntersticeValue::U64(current))
            }
            AutoIncState::I32(value) => {
                let current = *value;
                *value = value
                    .checked_add(1)
                    .ok_or_else(|| IntersticeError::Internal("auto_inc overflow".into()))?;
                Ok(IntersticeValue::I32(current))
            }
            AutoIncState::I64(value) => {
                let current = *value;
                *value = value
                    .checked_add(1)
                    .ok_or_else(|| IntersticeError::Internal("auto_inc overflow".into()))?;
                Ok(IntersticeValue::I64(current))
            }
        }
    }

    pub fn sync_from_value(&mut self, value: &IntersticeValue) -> Result<(), IntersticeError> {
        match (self, value) {
            (AutoIncState::U8(next), IntersticeValue::U8(v)) => {
                let candidate = v
                    .checked_add(1)
                    .ok_or_else(|| IntersticeError::Internal("auto_inc overflow".into()))?;
                if candidate > *next {
                    *next = candidate;
                }
                Ok(())
            }
            (AutoIncState::U32(next), IntersticeValue::U32(v)) => {
                let candidate = v
                    .checked_add(1)
                    .ok_or_else(|| IntersticeError::Internal("auto_inc overflow".into()))?;
                if candidate > *next {
                    *next = candidate;
                }
                Ok(())
            }
            (AutoIncState::U64(next), IntersticeValue::U64(v)) => {
                let candidate = v
                    .checked_add(1)
                    .ok_or_else(|| IntersticeError::Internal("auto_inc overflow".into()))?;
                if candidate > *next {
                    *next = candidate;
                }
                Ok(())
            }
            (AutoIncState::I32(next), IntersticeValue::I32(v)) => {
                let candidate = v
                    .checked_add(1)
                    .ok_or_else(|| IntersticeError::Internal("auto_inc overflow".into()))?;
                if candidate > *next {
                    *next = candidate;
                }
                Ok(())
            }
            (AutoIncState::I64(next), IntersticeValue::I64(v)) => {
                let candidate = v
                    .checked_add(1)
                    .ok_or_else(|| IntersticeError::Internal("auto_inc overflow".into()))?;
                if candidate > *next {
                    *next = candidate;
                }
                Ok(())
            }
            _ => Err(IntersticeError::Internal(
                "auto_inc value type mismatch".into(),
            )),
        }
    }
}

impl Table {


    pub(crate) fn auto_inc_snapshot(&self) -> TableAutoIncSnapshot {
        TableAutoIncSnapshot {
            primary: self.primary_key_auto_inc_state.clone(),
            indexes: self
                .indexes
                .iter()
                .map(|index| {
                    if index.auto_inc {
                        index.auto_inc_state.clone()
                    } else {
                        None
                    }
                })
                .collect(),
        }
    }

    pub(crate) fn apply_auto_inc_from_snapshot(
        &self,
        row: &mut Row,
        snapshot: &mut TableAutoIncSnapshot,
    ) -> Result<(), IntersticeError> {
        if self.primary_key_auto_inc {
            let state = snapshot.primary.as_mut().ok_or_else(|| {
                IntersticeError::Internal(format!(
                    "auto_inc is not supported for primary key in table '{}'",
                    self.schema.name
                ))
            })?;
            row.primary_key = state.next_value()?;
        }

        for (index, table_index) in self.indexes.iter().enumerate() {
            if !table_index.auto_inc {
                continue;
            }
            let state = snapshot
                .indexes
                .get_mut(index)
                .and_then(|s| s.as_mut())
                .ok_or_else(|| {
                    IntersticeError::Internal(format!(
                        "auto_inc is not supported for field '{}' in table '{}'",
                        table_index.field_name, self.schema.name
                    ))
                })?;
            let value = state.next_value()?;
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

    pub(crate) fn sync_auto_inc_from_row(&mut self, row: &Row) -> Result<(), IntersticeError> {
        if self.primary_key_auto_inc {
            let state = self.primary_key_auto_inc_state.as_mut().ok_or_else(|| {
                IntersticeError::Internal(format!(
                    "auto_inc is not supported for primary key in table '{}'",
                    self.schema.name
                ))
            })?;
            state.sync_from_value(&row.primary_key)?;
        }
        for table_index in &mut self.indexes {
            table_index.sync_auto_inc_from_row(row, &self.schema.name)?;
        }
        Ok(())
    }
}