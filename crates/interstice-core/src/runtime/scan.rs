// Efficient table scanning with iterator patterns and predicate pushdown
use crate::runtime::table::Table;
use interstice_abi::Row;

/// Iterator over table rows without allocating the entire table
pub struct TableIterator<'a> {
    rows: std::slice::Iter<'a, Row>,
}

impl<'a> TableIterator<'a> {
    pub fn new(table: &'a Table) -> Self {
        TableIterator {
            rows: table.rows.iter(),
        }
    }
}

impl<'a> Iterator for TableIterator<'a> {
    type Item = &'a Row;

    fn next(&mut self) -> Option<Self::Item> {
        self.rows.next()
    }
}

/// Predicate-based filter for efficient table scans
/// Applies filters during iteration rather than after collecting all rows
pub struct FilteredTableIterator<'a, F>
where
    F: Fn(&Row) -> bool,
{
    rows: std::slice::Iter<'a, Row>,
    predicate: F,
}

impl<'a, F> FilteredTableIterator<'a, F>
where
    F: Fn(&Row) -> bool,
{
    pub fn new(table: &'a Table, predicate: F) -> Self {
        FilteredTableIterator {
            rows: table.rows.iter(),
            predicate,
        }
    }
}

impl<'a, F> Iterator for FilteredTableIterator<'a, F>
where
    F: Fn(&Row) -> bool,
{
    type Item = &'a Row;

    fn next(&mut self) -> Option<Self::Item> {
        self.rows.by_ref().find(|row| (self.predicate)(row))
    }
}

/// Efficient indexed scan using secondary index
/// Returns row indices matched by the index
pub struct IndexedScan {
    matched_indices: Vec<usize>,
    current: usize,
}

impl IndexedScan {
    pub fn new(matched_indices: Vec<usize>) -> Self {
        IndexedScan {
            matched_indices,
            current: 0,
        }
    }

    pub fn matched_count(&self) -> usize {
        self.matched_indices.len()
    }

    pub fn get_indices(&self) -> &[usize] {
        &self.matched_indices
    }
}

impl Iterator for IndexedScan {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current < self.matched_indices.len() {
            let idx = self.matched_indices[self.current];
            self.current += 1;
            Some(idx)
        } else {
            None
        }
    }
}

/// Range scan for sorted indexes
/// Useful for range queries on indexed columns
pub struct RangeScan {
    matched_indices: Vec<usize>,
    current: usize,
}

impl RangeScan {
    pub fn new(matched_indices: Vec<usize>) -> Self {
        RangeScan {
            matched_indices,
            current: 0,
        }
    }

    pub fn count(&self) -> usize {
        self.matched_indices.len()
    }
}

impl Iterator for RangeScan {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current < self.matched_indices.len() {
            let idx = self.matched_indices[self.current];
            self.current += 1;
            Some(idx)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use interstice_abi::{Row, IntersticeValue, schema::{TableSchema, TableVisibility, EntrySchema}, IntersticeType};

    fn create_test_schema() -> TableSchema {
        TableSchema {
            name: "test".to_string(),
            visibility: TableVisibility::Public,
            primary_key: EntrySchema {
                name: "id".to_string(),
                value_type: IntersticeType::U64,
            },
            entries: vec![
                EntrySchema {
                    name: "value".to_string(),
                    value_type: IntersticeType::U32,
                },
            ],
        }
    }

    #[test]
    fn test_table_iterator() {
        let schema = create_test_schema();
        let mut table = Table::new(schema);
        
        for i in 1..=5 {
            let row = Row {
                primary_key: IntersticeValue::U64(i),
                entries: vec![IntersticeValue::U32(i as u32 * 10)],
            };
            table.insert(row).ok();
        }

        let iter = TableIterator::new(&table);
        assert_eq!(iter.count(), 5);
    }

    #[test]
    fn test_filtered_iterator() {
        let schema = create_test_schema();
        let mut table = Table::new(schema);
        
        for i in 1..=10 {
            let row = Row {
                primary_key: IntersticeValue::U64(i),
                entries: vec![IntersticeValue::U32(i as u32)],
            };
            table.insert(row).ok();
        }

        // Filter for rows with value > 5
        let filtered = FilteredTableIterator::new(&table, |row| {
            if let IntersticeValue::U32(val) = row.entries[0] {
                val > 5
            } else {
                false
            }
        });

        let count = filtered.count();
        assert_eq!(count, 5); // Rows 6-10
    }

    #[test]
    fn test_indexed_scan() {
        let indices = vec![0, 2, 4, 6];
        let scan = IndexedScan::new(indices);
        assert_eq!(scan.matched_count(), 4);
        assert_eq!(scan.get_indices(), &[0, 2, 4, 6]);
    }

    #[test]
    fn test_indexed_scan_iteration() {
        let indices = vec![1, 3, 5];
        let scan = IndexedScan::new(indices);
        let collected: Vec<usize> = scan.collect();
        assert_eq!(collected, vec![1, 3, 5]);
    }

    #[test]
    fn test_range_scan() {
        let indices = vec![10, 20, 30, 40, 50];
        let scan = RangeScan::new(indices);
        assert_eq!(scan.count(), 5);
    }

    #[test]
    fn test_range_scan_iteration() {
        let indices = vec![10, 20, 30, 40, 50];
        let scan = RangeScan::new(indices);
        let collected: Vec<usize> = scan.collect();
        assert_eq!(collected, vec![10, 20, 30, 40, 50]);
    }

    #[test]
    fn test_filtered_iterator_empty_result() {
        let schema = create_test_schema();
        let mut table = Table::new(schema);
        
        for i in 1..=5 {
            let row = Row {
                primary_key: IntersticeValue::U64(i),
                entries: vec![IntersticeValue::U32(i as u32)],
            };
            table.insert(row).ok();
        }

        // Filter for rows with value > 100 (none match)
        let filtered = FilteredTableIterator::new(&table, |row| {
            if let IntersticeValue::U32(val) = row.entries[0] {
                val > 100
            } else {
                false
            }
        });

        assert_eq!(filtered.count(), 0);
    }

    #[test]
    fn test_multiple_filters() {
        let schema = create_test_schema();
        let mut table = Table::new(schema);
        
        for i in 1..=20 {
            let row = Row {
                primary_key: IntersticeValue::U64(i),
                entries: vec![IntersticeValue::U32(i as u32)],
            };
            table.insert(row).ok();
        }

        // Chain filters: value > 5 AND value < 15
        let iter = TableIterator::new(&table);
        let count = iter
            .filter(|row| {
                if let IntersticeValue::U32(val) = row.entries[0] {
                    val > 5 && val < 15
                } else {
                    false
                }
            })
            .count();
        
        assert_eq!(count, 9); // Values 6-14
    }
}
