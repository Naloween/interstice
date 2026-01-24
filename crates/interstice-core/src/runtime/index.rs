use std::collections::BTreeMap;

/// Primary key index mapping RowId -> Row reference
/// Enables O(log n) lookups by primary key
pub struct PrimaryKeyIndex {
    // Maps serialized primary key to row index in table
    map: BTreeMap<Vec<u8>, usize>,
}

impl PrimaryKeyIndex {
    pub fn new() -> Self {
        PrimaryKeyIndex {
            map: BTreeMap::new(),
        }
    }

    /// Insert a row reference by its primary key
    pub fn insert(&mut self, key_bytes: Vec<u8>, row_index: usize) {
        self.map.insert(key_bytes, row_index);
    }

    /// Remove a row reference by its primary key
    pub fn remove(&mut self, key_bytes: &[u8]) -> Option<usize> {
        self.map.remove(key_bytes)
    }

    /// Lookup a row by its primary key
    pub fn get(&self, key_bytes: &[u8]) -> Option<usize> {
        self.map.get(key_bytes).copied()
    }

    /// Update the row index for a key (used when row array is modified)
    pub fn update(&mut self, key_bytes: &[u8], new_row_index: usize) -> bool {
        if let Some(entry) = self.map.get_mut(key_bytes) {
            *entry = new_row_index;
            true
        } else {
            false
        }
    }

    /// Number of indexed rows
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Get all row indices in order
    pub fn all_indices(&self) -> Vec<usize> {
        self.map.values().copied().collect()
    }
}

/// Secondary index on a single column
pub struct SecondaryIndex {
    // Maps column value -> Vec<row indices>
    // Using Vec because multiple rows can have same value
    map: BTreeMap<Vec<u8>, Vec<usize>>,
    column_name: String,
}

impl SecondaryIndex {
    pub fn new(column_name: String) -> Self {
        SecondaryIndex {
            map: BTreeMap::new(),
            column_name,
        }
    }

    pub fn column_name(&self) -> &str {
        &self.column_name
    }

    /// Add a row reference to the index
    pub fn insert(&mut self, value_bytes: Vec<u8>, row_index: usize) {
        self.map
            .entry(value_bytes)
            .or_insert_with(Vec::new)
            .push(row_index);
    }

    /// Remove a row reference from the index
    pub fn remove(&mut self, value_bytes: &[u8], row_index: usize) -> bool {
        if let Some(indices) = self.map.get_mut(value_bytes) {
            indices.retain(|&idx| idx != row_index);
            indices.is_empty() && self.map.remove(value_bytes).is_some()
        } else {
            false
        }
    }

    /// Find all rows with a specific column value
    pub fn query(&self, value_bytes: &[u8]) -> Vec<usize> {
        self.map.get(value_bytes).cloned().unwrap_or_default()
    }

    /// Find all rows with column value in a range (for comparable types)
    pub fn range_query(&self, start: &[u8], end: &[u8]) -> Vec<usize> {
        let mut result = Vec::new();
        for (_, indices) in self.map.range(start.to_vec()..=end.to_vec()) {
            result.extend(indices);
        }
        result
    }

    /// Number of unique values indexed
    pub fn value_count(&self) -> usize {
        self.map.len()
    }

    /// Total number of entries (including duplicates)
    pub fn total_entries(&self) -> usize {
        self.map.values().map(|v| v.len()).sum()
    }
}

/// Composite index on multiple columns
pub struct CompositeIndex {
    // Maps tuple of (col1_bytes, col2_bytes, ...) -> Vec<row indices>
    map: BTreeMap<Vec<Vec<u8>>, Vec<usize>>,
    column_names: Vec<String>,
}

impl CompositeIndex {
    pub fn new(column_names: Vec<String>) -> Self {
        CompositeIndex {
            map: BTreeMap::new(),
            column_names,
        }
    }

    pub fn column_names(&self) -> &[String] {
        &self.column_names
    }

    /// Insert a row reference with multiple column values
    pub fn insert(&mut self, value_tuple: Vec<Vec<u8>>, row_index: usize) {
        assert_eq!(
            value_tuple.len(),
            self.column_names.len(),
            "Value tuple length must match column count"
        );
        self.map
            .entry(value_tuple)
            .or_insert_with(Vec::new)
            .push(row_index);
    }

    /// Remove a row reference
    pub fn remove(&mut self, value_tuple: &[Vec<u8>], row_index: usize) -> bool {
        if let Some(indices) = self.map.get_mut(value_tuple) {
            indices.retain(|&idx| idx != row_index);
            indices.is_empty() && self.map.remove(value_tuple).is_some()
        } else {
            false
        }
    }

    /// Query with exact tuple match
    pub fn query(&self, value_tuple: &[Vec<u8>]) -> Vec<usize> {
        self.map.get(value_tuple).cloned().unwrap_or_default()
    }

    /// Number of unique tuples indexed
    pub fn tuple_count(&self) -> usize {
        self.map.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_primary_key_index_insert_get() {
        let mut index = PrimaryKeyIndex::new();
        let key = b"key1".to_vec();
        index.insert(key.clone(), 0);
        assert_eq!(index.get(&key), Some(0));
    }

    #[test]
    fn test_primary_key_index_remove() {
        let mut index = PrimaryKeyIndex::new();
        let key = b"key1".to_vec();
        index.insert(key.clone(), 0);
        assert_eq!(index.remove(&key), Some(0));
        assert_eq!(index.get(&key), None);
    }

    #[test]
    fn test_primary_key_index_update() {
        let mut index = PrimaryKeyIndex::new();
        let key = b"key1".to_vec();
        index.insert(key.clone(), 0);
        assert!(index.update(&key, 1));
        assert_eq!(index.get(&key), Some(1));
    }

    #[test]
    fn test_secondary_index_insert_query() {
        let mut index = SecondaryIndex::new("email".to_string());
        let value = b"user@example.com".to_vec();
        index.insert(value.clone(), 0);
        index.insert(value.clone(), 1);
        assert_eq!(index.query(&value), vec![0, 1]);
    }

    #[test]
    fn test_secondary_index_remove() {
        let mut index = SecondaryIndex::new("email".to_string());
        let value = b"user@example.com".to_vec();
        index.insert(value.clone(), 0);
        index.insert(value.clone(), 1);
        index.remove(&value, 0);
        assert_eq!(index.query(&value), vec![1]);
    }

    #[test]
    fn test_secondary_index_range_query() {
        let mut index = SecondaryIndex::new("age".to_string());
        for i in 20..=30 {
            let key = vec![i as u8];
            index.insert(key, i as usize);
        }
        let results = index.range_query(&vec![25], &vec![27]);
        assert!(results.len() > 0);
    }

    #[test]
    fn test_composite_index_insert_query() {
        let mut index = CompositeIndex::new(vec!["first_name".to_string(), "last_name".to_string()]);
        let key = vec![b"John".to_vec(), b"Doe".to_vec()];
        index.insert(key.clone(), 0);
        assert_eq!(index.query(&key), vec![0]);
    }

    #[test]
    fn test_composite_index_multiple_rows() {
        let mut index = CompositeIndex::new(vec!["dept".to_string(), "role".to_string()]);
        let key = vec![b"engineering".to_vec(), b"lead".to_vec()];
        index.insert(key.clone(), 0);
        index.insert(key.clone(), 1);
        assert_eq!(index.query(&key), vec![0, 1]);
    }
}
