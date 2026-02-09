use interstice_abi::{IndexKey, IntersticeValue, Row, decode, encode};
use serde::{Deserialize, Serialize};

use crate::error::IntersticeError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Transaction {
    Insert {
        module_name: String,
        table_name: String,
        new_row: Row,
    },
    Update {
        module_name: String,
        table_name: String,
        update_row: Row,
    },
    Delete {
        module_name: String,
        table_name: String,
        deleted_row_id: IndexKey,
    },
}

impl Transaction {
    pub fn encode(&self) -> Result<Vec<u8>, IntersticeError> {
        encode(&self).map_err(|err| {
            IntersticeError::Internal(format!("Couldn't encode transaction: {}", err))
        })
    }
    pub fn decode(bytes: &[u8]) -> Result<Transaction, IntersticeError> {
        decode(bytes).map_err(|err| {
            IntersticeError::Internal(format!("Couldn't decode transaction: {}", err))
        })
    }
}
