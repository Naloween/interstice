use interstice_abi::{
    IndexKey, IndexQuery, IndexSchema, IndexType, IntersticeType, IntersticeValue, Row, TableSchema,
};
use std::collections::BTreeMap;
use wgpu::naga::FastHashMap;

use crate::IntersticeError;

pub struct Table {
    pub schema: TableSchema,
    rows: Vec<Row>,
    primary_key_index: FastHashMap<IndexKey, usize>,
    indexes: Vec<TableIndex>,
    primary_key_auto_inc: bool,
    primary_key_auto_inc_state: Option<AutoIncState>,
}

struct TableIndex {
    field_name: String,
    field_index: usize,
    unique: bool,
    auto_inc: bool,
    auto_inc_state: Option<AutoIncState>,
    index: IndexImpl,
}

enum AutoIncState {
    U8(u8),
    U32(u32),
    U64(u64),
    I32(i32),
    I64(i64),
}

enum IndexImpl {
    Hash(FastHashMap<IndexKey, Vec<usize>>),
    BTree(BTreeMap<IndexKey, Vec<usize>>),
}

impl AutoIncState {
    fn from_type(field_type: &IntersticeType) -> Option<Self> {
        match field_type {
            IntersticeType::U8 => Some(Self::U8(0)),
            IntersticeType::U32 => Some(Self::U32(0)),
            IntersticeType::U64 => Some(Self::U64(0)),
            IntersticeType::I32 => Some(Self::I32(0)),
            IntersticeType::I64 => Some(Self::I64(0)),
            _ => None,
        }
    }

    fn next_value(&mut self) -> Result<IntersticeValue, IntersticeError> {
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

    fn sync_from_value(&mut self, value: &IntersticeValue) -> Result<(), IntersticeError> {
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
    pub fn new(schema: TableSchema) -> Self {
        let field_positions = schema
            .fields
            .iter()
            .enumerate()
            .map(|(index, field)| (field.name.clone(), index))
            .collect::<FastHashMap<_, _>>();

        let indexes = schema
            .indexes
            .iter()
            .filter_map(|index_schema| {
                field_positions
                    .get(&index_schema.field_name)
                    .and_then(|&field_index| {
                        schema.fields.get(field_index).map(|field_def| {
                            TableIndex::new(index_schema, field_index, &field_def.field_type)
                        })
                    })
            })
            .collect();
        let primary_key_auto_inc = schema.primary_key_auto_inc;
        let primary_key_auto_inc_state = if primary_key_auto_inc {
            AutoIncState::from_type(&schema.primary_key.field_type)
        } else {
            None
        };

        Self {
            schema,
            rows: Vec::new(),
            primary_key_index: FastHashMap::default(),
            indexes,
            primary_key_auto_inc,
            primary_key_auto_inc_state,
        }
    }

    pub fn prepare_insert(&mut self, mut row: Row) -> Result<Row, IntersticeError> {
        if self.primary_key_auto_inc {
            let state = self.primary_key_auto_inc_state.as_mut().ok_or_else(|| {
                IntersticeError::Internal(format!(
                    "auto_inc is not supported for primary key in table '{}'",
                    self.schema.name
                ))
            })?;
            row.primary_key = state.next_value()?;
        }
        for table_index in &mut self.indexes {
            table_index.apply_auto_inc(&mut row, &self.schema.name)?;
        }
        Ok(row)
    }

    pub fn validate_insert(&self, row: &Row) -> Result<(), IntersticeError> {
        let primary_key_value: IndexKey = row
            .primary_key
            .clone()
            .try_into()
            .map_err(|err| IntersticeError::Internal(err))?;

        if self.primary_key_index.contains_key(&primary_key_value) {
            return Err(IntersticeError::UniqueConstraintViolation {
                table_name: self.schema.name.clone(),
                field_name: self.schema.primary_key.name.clone(),
            });
        }

        for table_index in &self.indexes {
            if table_index.unique {
                let key = table_index.key_from_row(row)?;
                if table_index.contains_key(&key) {
                    return Err(IntersticeError::UniqueConstraintViolation {
                        table_name: self.schema.name.clone(),
                        field_name: table_index.field_name.clone(),
                    });
                }
            }
        }

        Ok(())
    }

    pub fn insert(&mut self, row: Row) -> Result<(), IntersticeError> {
        let row = row;
        self.sync_auto_inc_from_row(&row)?;

        let index = self.rows.len();
        let primary_key_value: IndexKey = row
            .primary_key
            .clone()
            .try_into()
            .map_err(|err| IntersticeError::Internal(err))?;

        if self.primary_key_index.contains_key(&primary_key_value) {
            return Err(IntersticeError::UniqueConstraintViolation {
                table_name: self.schema.name.clone(),
                field_name: self.schema.primary_key.name.clone(),
            });
        }

        for table_index in &mut self.indexes {
            let key = table_index.key_from_row(&row)?;
            table_index.insert(index, key, &self.schema.name)?;
        }

        self.primary_key_index.insert(primary_key_value, index);
        self.rows.push(row);
        Ok(())
    }

    pub fn update(&mut self, row: Row) -> Result<Row, IntersticeError> {
        let primary_key_value: IndexKey = row
            .primary_key
            .clone()
            .try_into()
            .map_err(|err| IntersticeError::Internal(err))?;
        if let Some(&index) = self.primary_key_index.get(&primary_key_value) {
            let old_row = std::mem::replace(&mut self.rows[index], row);
            for table_index in &mut self.indexes {
                let old_key = table_index.key_from_row(&old_row)?;
                let new_key = table_index.key_from_row(&self.rows[index])?;
                table_index.update(index, old_key, new_key, &self.schema.name)?;
            }
            Ok(old_row)
        } else {
            Err(IntersticeError::RowNotFound { primary_key_value })
        }
    }

    pub fn delete(&mut self, primary_key_value: &IndexKey) -> Result<Row, IntersticeError> {
        if let Some(&index) = self.primary_key_index.get(primary_key_value) {
            let deleted_row = self.rows.swap_remove(index);
            self.primary_key_index.remove(primary_key_value);

            for table_index in &mut self.indexes {
                let deleted_key = table_index.key_from_row(&deleted_row)?;
                table_index.remove(index, deleted_key);
            }

            // Update the swapped row's index in the primary key index
            if index < self.rows.len() {
                let swapped_row = &self.rows[index];
                let swapped_primary_key_value: IndexKey = swapped_row
                    .primary_key
                    .clone()
                    .try_into()
                    .map_err(|err| IntersticeError::Internal(err))?;
                self.primary_key_index
                    .insert(swapped_primary_key_value, index);

                let swapped_position = self.rows.len();
                for table_index in &mut self.indexes {
                    let swapped_key = table_index.key_from_row(swapped_row)?;
                    table_index.replace_position(swapped_position, index, swapped_key);
                }
            }
            Ok(deleted_row)
        } else {
            Err(IntersticeError::RowNotFound {
                primary_key_value: primary_key_value.clone(),
            })
        }
    }

    pub fn get_by_primary_key(&self, primary_key_value: &IndexKey) -> Option<&Row> {
        self.primary_key_index
            .get(primary_key_value)
            .map(|&index| &self.rows[index])
    }

    pub fn scan(&self) -> &[Row] {
        &self.rows
    }

    pub fn get_by_index(
        &self,
        field_name: &str,
        query: &IndexQuery,
    ) -> Result<Vec<&Row>, IntersticeError> {
        let table_index = self
            .indexes
            .iter()
            .find(|index| index.field_name == field_name)
            .ok_or_else(|| IntersticeError::IndexNotFound {
                table_name: self.schema.name.clone(),
                field_name: field_name.to_string(),
            })?;

        let positions = table_index.scan(query, &self.schema.name)?;
        Ok(positions
            .into_iter()
            .filter_map(|pos| self.rows.get(pos))
            .collect())
    }

    fn sync_auto_inc_from_row(&mut self, row: &Row) -> Result<(), IntersticeError> {
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

impl TableIndex {
    fn new(schema: &IndexSchema, field_index: usize, field_type: &IntersticeType) -> Self {
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

    fn key_from_row(&self, row: &Row) -> Result<IndexKey, IntersticeError> {
        row.entries
            .get(self.field_index)
            .ok_or_else(|| IntersticeError::Internal("Index field out of bounds".to_string()))
            .and_then(|value| value.clone().try_into().map_err(IntersticeError::Internal))
    }

    fn apply_auto_inc(&mut self, row: &mut Row, table_name: &str) -> Result<(), IntersticeError> {
        if !self.auto_inc {
            return Ok(());
        }
        let state = self.auto_inc_state.as_mut().ok_or_else(|| {
            IntersticeError::Internal(format!(
                "auto_inc is not supported for field '{}' in table '{}'",
                self.field_name, table_name
            ))
        })?;
        let value = state.next_value()?;
        if let Some(entry) = row.entries.get_mut(self.field_index) {
            *entry = value;
            Ok(())
        } else {
            Err(IntersticeError::Internal(
                "Index field out of bounds".to_string(),
            ))
        }
    }

    fn sync_auto_inc_from_row(
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

    fn contains_key(&self, key: &IndexKey) -> bool {
        match &self.index {
            IndexImpl::Hash(map) => map.contains_key(key),
            IndexImpl::BTree(map) => map.contains_key(key),
        }
    }

    fn insert(
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

    fn update(
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

    fn remove(&mut self, position: usize, key: IndexKey) {
        match &mut self.index {
            IndexImpl::Hash(map) => Self::remove_from_hash(map, position, &key),
            IndexImpl::BTree(map) => Self::remove_from_btree(map, position, &key),
        }
    }

    fn replace_position(&mut self, old_position: usize, new_position: usize, key: IndexKey) {
        match &mut self.index {
            IndexImpl::Hash(map) => Self::replace_in_hash(map, old_position, new_position, &key),
            IndexImpl::BTree(map) => Self::replace_in_btree(map, old_position, new_position, &key),
        }
    }

    fn scan(&self, query: &IndexQuery, table_name: &str) -> Result<Vec<usize>, IntersticeError> {
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
