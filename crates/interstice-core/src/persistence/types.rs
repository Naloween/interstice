//! Transaction types for the persistence log

use interstice_abi::Row;
use serde::{Deserialize, Serialize};

/// Binary log format version (bump when format changes)
pub const LOG_FORMAT_VERSION: u8 = 1;

/// Represents a single mutation transaction in the log
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Transaction {
    /// Type of mutation (Insert, Update, Delete)
    pub transaction_type: TransactionType,
    /// Module name that owns the table
    pub module_name: String,
    /// Table name
    pub table_name: String,
    /// Row being inserted, updated, or deleted
    pub row: Row,
    /// Previous row value (for Updates only)
    pub old_row: Option<Row>,
    /// Logical clock / timestamp
    pub timestamp: u64,
}

/// Type of table mutation
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum TransactionType {
    /// Insert a new row
    Insert = 1,
    /// Update an existing row
    Update = 2,
    /// Delete an existing row
    Delete = 3,
}

impl TransactionType {
    /// Parse a transaction type from its byte representation
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            1 => Some(TransactionType::Insert),
            2 => Some(TransactionType::Update),
            3 => Some(TransactionType::Delete),
            _ => None,
        }
    }

    /// Convert to byte representation
    pub fn to_byte(self) -> u8 {
        self as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_type_roundtrip() {
        for ty in &[TransactionType::Insert, TransactionType::Update, TransactionType::Delete] {
            let byte = ty.to_byte();
            assert_eq!(TransactionType::from_byte(byte), Some(*ty));
        }
    }
}
