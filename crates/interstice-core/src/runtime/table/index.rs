use interstice_abi::{
    IndexKey, IndexQuery, IndexSchema, IndexType, IntersticeType, Row  
};
use std::collections::BTreeMap;
use wgpu::naga::FastHashMap;
use crate::{IntersticeError, runtime::table::auto_inc::{AutoIncState, IndexImpl}};

pub struct TableIndex {
    pub field_name: String,
    pub field_index: usize,
    pub unique: bool,
    pub auto_inc: bool,
    pub auto_inc_state: Option<AutoIncState>,
    pub index: IndexImpl,
}


impl TableIndex {
    pub fn new(schema: &IndexSchema, field_index: usize, field_type: &IntersticeType) -> Self {
        let index = match schema.index_type {
            IndexType::Hash => IndexImpl::Hash(FastHashMap::default()),
            IndexType::BTree => IndexImpl::BTree(BTreeMap::new()),
        };
        let auto_inc_state = if schema.auto_inc {
            AutoIncState::from_type(field_type)
        } else {
            None
        };
        Self {
            field_name: schema.field_name.clone(),
            field_index,
            unique: schema.unique || schema.auto_inc,
            auto_inc: schema.auto_inc,
            auto_inc_state,
            index,
        }
    }

    pub fn key_from_row(&self, row: &Row) -> Result<IndexKey, IntersticeError> {
        row.entries
            .get(self.field_index)
            .ok_or_else(|| IntersticeError::Internal("Index field out of bounds".to_string()))
            .and_then(|value| value.clone().try_into().map_err(IntersticeError::Internal))
    }

    pub fn sync_auto_inc_from_row(
        &mut self,
        row: &Row,
        table_name: &str,
    ) -> Result<(), IntersticeError> {
        if !self.auto_inc {
            return Ok(());
        }
        let state = self.auto_inc_state.as_mut().ok_or_else(|| {
            IntersticeError::Internal(format!(
                "auto_inc is not supported for field '{}' in table '{}'",
                self.field_name, table_name
            ))
        })?;
        let value = row
            .entries
            .get(self.field_index)
            .ok_or_else(|| IntersticeError::Internal("Index field out of bounds".to_string()))?;
        state.sync_from_value(value)
    }

    pub fn contains_key(&self, key: &IndexKey) -> bool {
        match &self.index {
            IndexImpl::Hash(map) => map.contains_key(key),
            IndexImpl::BTree(map) => map.contains_key(key),
        }
    }

    pub fn positions(&self, key: &IndexKey) -> Option<&Vec<usize>> {
        match &self.index {
            IndexImpl::Hash(map) => map.get(key),
            IndexImpl::BTree(map) => map.get(key),
        }
    }

    pub fn insert(
        &mut self,
        position: usize,
        key: IndexKey,
        table_name: &str,
    ) -> Result<(), IntersticeError> {
        match &mut self.index {
            IndexImpl::Hash(map) => Self::insert_into_hash(
                map,
                position,
                key,
                self.unique,
                table_name,
                &self.field_name,
            ),
            IndexImpl::BTree(map) => Self::insert_into_btree(
                map,
                position,
                key,
                self.unique,
                table_name,
                &self.field_name,
            ),
        }
    }

    pub fn update(
        &mut self,
        position: usize,
        old_key: IndexKey,
        new_key: IndexKey,
        table_name: &str,
    ) -> Result<(), IntersticeError> {
        if old_key == new_key {
            return Ok(());
        }
        if self.auto_inc {
            return Err(IntersticeError::AutoIncUpdateNotAllowed {
                table_name: table_name.to_string(),
                field_name: self.field_name.clone(),
            });
        }
        self.insert(position, new_key, table_name)?;
        self.remove(position, old_key);
        Ok(())
    }

    pub fn remove(&mut self, position: usize, key: IndexKey) {
        match &mut self.index {
            IndexImpl::Hash(map) => Self::remove_from_hash(map, position, &key),
            IndexImpl::BTree(map) => Self::remove_from_btree(map, position, &key),
        }
    }

    pub fn replace_position(&mut self, old_position: usize, new_position: usize, key: IndexKey) {
        match &mut self.index {
            IndexImpl::Hash(map) => Self::replace_in_hash(map, old_position, new_position, &key),
            IndexImpl::BTree(map) => Self::replace_in_btree(map, old_position, new_position, &key),
        }
    }

    pub fn scan(&self, query: &IndexQuery, table_name: &str) -> Result<Vec<usize>, IntersticeError> {
        match (&self.index, query) {
            (IndexImpl::Hash(map), IndexQuery::Eq(key)) => {
                Ok(map.get(key).cloned().unwrap_or_default())
            }
            (IndexImpl::Hash(_), _) => Err(IntersticeError::IndexQueryUnsupported {
                table_name: table_name.to_string(),
                field_name: self.field_name.clone(),
            }),
            (IndexImpl::BTree(map), IndexQuery::Eq(key)) => {
                Ok(map.get(key).cloned().unwrap_or_default())
            }
            (IndexImpl::BTree(map), IndexQuery::Lt(key)) => Ok(map
                .range(..key.clone())
                .flat_map(|(_, v)| v.clone())
                .collect()),
            (IndexImpl::BTree(map), IndexQuery::Lte(key)) => Ok(map
                .range(..=key.clone())
                .flat_map(|(_, v)| v.clone())
                .collect()),
            (IndexImpl::BTree(map), IndexQuery::Gt(key)) => Ok(map
                .range((
                    std::ops::Bound::Excluded(key.clone()),
                    std::ops::Bound::Unbounded,
                ))
                .flat_map(|(_, v)| v.clone())
                .collect()),
            (IndexImpl::BTree(map), IndexQuery::Gte(key)) => Ok(map
                .range((
                    std::ops::Bound::Included(key.clone()),
                    std::ops::Bound::Unbounded,
                ))
                .flat_map(|(_, v)| v.clone())
                .collect()),
            (
                IndexImpl::BTree(map),
                IndexQuery::Range {
                    min,
                    max,
                    include_min,
                    include_max,
                },
            ) => {
                use std::ops::Bound::{Excluded, Included};
                let lower = if *include_min {
                    Included(min.clone())
                } else {
                    Excluded(min.clone())
                };
                let upper = if *include_max {
                    Included(max.clone())
                } else {
                    Excluded(max.clone())
                };
                Ok(map
                    .range((lower, upper))
                    .flat_map(|(_, v)| v.clone())
                    .collect())
            }
        }
    }

    fn insert_into_hash(
        map: &mut FastHashMap<IndexKey, Vec<usize>>,
        position: usize,
        key: IndexKey,
        unique: bool,
        table_name: &str,
        field_name: &str,
    ) -> Result<(), IntersticeError> {
        if unique {
            if let Some(existing) = map.get(&key) {
                if !existing.is_empty() {
                    return Err(IntersticeError::UniqueConstraintViolation {
                        table_name: table_name.to_string(),
                        field_name: field_name.to_string(),
                    });
                }
            }
        }
        map.entry(key).or_default().push(position);
        Ok(())
    }

    fn insert_into_btree(
        map: &mut BTreeMap<IndexKey, Vec<usize>>,
        position: usize,
        key: IndexKey,
        unique: bool,
        table_name: &str,
        field_name: &str,
    ) -> Result<(), IntersticeError> {
        if unique {
            if let Some(existing) = map.get(&key) {
                if !existing.is_empty() {
                    return Err(IntersticeError::UniqueConstraintViolation {
                        table_name: table_name.to_string(),
                        field_name: field_name.to_string(),
                    });
                }
            }
        }
        map.entry(key).or_default().push(position);
        Ok(())
    }

    fn remove_from_hash(
        map: &mut FastHashMap<IndexKey, Vec<usize>>,
        position: usize,
        key: &IndexKey,
    ) {
        if let Some(positions) = map.get_mut(key) {
            if let Some(pos_index) = positions.iter().position(|&p| p == position) {
                positions.swap_remove(pos_index);
            }
            if positions.is_empty() {
                map.remove(key);
            }
        }
    }

    fn remove_from_btree(
        map: &mut BTreeMap<IndexKey, Vec<usize>>,
        position: usize,
        key: &IndexKey,
    ) {
        if let Some(positions) = map.get_mut(key) {
            if let Some(pos_index) = positions.iter().position(|&p| p == position) {
                positions.swap_remove(pos_index);
            }
            if positions.is_empty() {
                map.remove(key);
            }
        }
    }

    fn replace_in_hash(
        map: &mut FastHashMap<IndexKey, Vec<usize>>,
        old_position: usize,
        new_position: usize,
        key: &IndexKey,
    ) {
        if let Some(positions) = map.get_mut(key) {
            if let Some(pos_index) = positions.iter().position(|&p| p == old_position) {
                positions[pos_index] = new_position;
            }
        }
    }

    fn replace_in_btree(
        map: &mut BTreeMap<IndexKey, Vec<usize>>,
        old_position: usize,
        new_position: usize,
        key: &IndexKey,
    ) {
        if let Some(positions) = map.get_mut(key) {
            if let Some(pos_index) = positions.iter().position(|&p| p == old_position) {
                positions[pos_index] = new_position;
            }
        }
    }
}
