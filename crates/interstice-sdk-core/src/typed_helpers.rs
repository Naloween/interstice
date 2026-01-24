//! Utilities for typed table operations in modules.
//!
//! This module provides helper functions and traits to simplify
//! working with typed tables without manually converting to/from IntersticeValue.

use crate::types::Serialize;
use interstice_abi::{IntersticeValue, Row};

/// Helper for inserting a typed row
///
/// Converts the typed data to IntersticeValue and calls the underlying insert.
#[cfg(target_arch = "wasm32")]
pub fn insert_typed_row<T: Serialize>(
    table_name: &str,
    primary_key: impl Serialize,
    data: T,
) -> std::result::Result<(), String> {
    let row = Row {
        primary_key: primary_key.to_value(),
        entries: vec![data.to_value()],
    };
    crate::host_calls::insert_row(table_name.to_string(), row);
    Ok(())
}

/// Helper for scanning a table and converting results to typed data
///
/// Returns all rows in the table, converted to the specified type.
#[cfg(target_arch = "wasm32")]
pub fn scan_typed<T: Serialize>(table_name: &str) -> Vec<T> {
    let raw_rows = crate::host_calls::scan(table_name.into());
    raw_rows
        .into_iter()
        .filter_map(|row| {
            // Extract first entry and convert
            row.entries.first().and_then(|v| T::from_value(v.clone()).ok())
        })
        .collect()
}

// Stub versions for testing
#[cfg(not(target_arch = "wasm32"))]
pub fn insert_typed_row<T: Serialize>(
    _table_name: &str,
    _primary_key: impl Serialize,
    _data: T,
) -> std::result::Result<(), String> {
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn scan_typed<T: Serialize>(_table_name: &str) -> Vec<T> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_helper_compiles() {
        // Just verify the helpers compile and type-check
        let _: std::result::Result<(), String> = insert_typed_row("test", 1u64, "hello".to_string());
    }
}
