//! Transaction types for the persistence log
//!
//! Defines the structure of transactions stored in the append-only log.
//! Each transaction records a single table mutation (insert, update, or delete).

use interstice_abi::Row;
use serde::{Deserialize, Serialize};

/// Binary log format version. Bump when format changes incompatibly.
pub const LOG_FORMAT_VERSION: u8 = 1;

/// A single table mutation recorded in the log.
///
/// Transactions are serialized to binary format with CRC32 checksums.
/// They capture the complete state needed to replay mutations without
/// re-executing reducer logic.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Transaction {
    pub transaction_type: TransactionType,
    pub module_name: String,
    pub table_name: String,
    pub row: Row,
    /// Used only for Update: the old row before modification
    pub old_row: Option<Row>,
    /// Logical timestamp for ordering during replay
    pub timestamp: u64,
}

/// The three types of table mutations
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum TransactionType {
    Insert = 1,
    Update = 2,
    Delete = 3,
}

impl TransactionType {
    /// Parse from byte representation (used during deserialization)
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            1 => Some(TransactionType::Insert),
            2 => Some(TransactionType::Update),
            3 => Some(TransactionType::Delete),
            _ => None,
        }
    }

    /// Convert to byte representation (used during serialization)
    pub fn to_byte(self) -> u8 {
        self as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_type_roundtrip() {
        for ty in &[
            TransactionType::Insert,
            TransactionType::Update,
            TransactionType::Delete,
        ] {
            let byte = ty.to_byte();
            assert_eq!(TransactionType::from_byte(byte), Some(*ty));
        }
    }
}
