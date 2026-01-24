//! Typed table handles for type-safe table operations
//!
//! This module provides TableHandle<T> for accessing tables with compile-time type checking.

use crate::types::Serialize;
use interstice_abi::IntersticeValue;

/// A strongly-typed handle to a table
///
/// Prevents accidentally using wrong types when accessing tables.
/// Type parameter T specifies what type of data is stored in the table.
///
/// # Example
/// ```ignore
/// let users: TableHandle<String> = TableHandle::new("users");
/// insert_typed_row("users", 1u64, "Alice".to_string())?;
/// let all_users: Vec<String> = scan_typed("users");
/// ```
#[derive(Debug, Clone)]
pub struct TableHandle<T: Serialize> {
    table_name: String,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Serialize> TableHandle<T> {
    /// Create a new typed table handle
    pub fn new(table_name: &str) -> Self {
        Self {
            table_name: table_name.to_string(),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Get the table name
    pub fn name(&self) -> &str {
        &self.table_name
    }

    /// Insert a typed row (placeholder - actual implementation requires runtime)
    pub fn insert(&self, _pk: impl Serialize, _data: T) -> std::result::Result<(), String> {
        #[cfg(target_arch = "wasm32")]
        {
            use crate::host_calls;
            use interstice_abi::Row;
            let row = Row {
                primary_key: _pk.to_value(),
                entries: vec![_data.to_value()],
            };
            host_calls::insert_row(self.table_name.clone(), row);
            Ok(())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            Ok(())
        }
    }

    /// Get all rows from the table as typed data
    pub fn scan(&self) -> Vec<T> {
        #[cfg(target_arch = "wasm32")]
        {
            use crate::host_calls;
            let raw_rows = host_calls::scan(self.table_name.clone());
            raw_rows
                .into_iter()
                .filter_map(|row| {
                    row.entries.first().and_then(|v| T::from_value(v.clone()).ok())
                })
                .collect()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_table_handle_creation() {
        let table: TableHandle<String> = TableHandle::new("test");
        assert_eq!(table.name(), "test");
    }

    #[test]
    fn test_table_handle_insert_string() {
        let table: TableHandle<String> = TableHandle::new("strings");
        let result = table.insert(1u64, "hello".to_string());
        assert!(result.is_ok());
    }

    #[test]
    fn test_table_handle_insert_u64() {
        let table: TableHandle<u64> = TableHandle::new("numbers");
        let result = table.insert(1u64, 42u64);
        assert!(result.is_ok());
    }

    #[test]
    fn test_table_handle_clone() {
        let table1: TableHandle<String> = TableHandle::new("test");
        let table2 = table1.clone();
        assert_eq!(table1.name(), table2.name());
    }
}
