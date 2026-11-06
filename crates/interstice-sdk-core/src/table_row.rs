//! Row struct metadata shared by `#[table]` and schema registration.

use interstice_abi::{ModuleSelection, NodeSelection, ReducerTableRef};

/// Implemented by every `#[table]` row type. Maps the Rust struct to the module table name string.
pub trait TableRow {
    const TABLE_NAME: &'static str;

    fn table_ref() -> ReducerTableRef {
        ReducerTableRef {
            node_selection: NodeSelection::Current,
            module_selection: ModuleSelection::Current,
            table_name: Self::TABLE_NAME.to_string(),
        }
    }
}

/// A table row written to and read from its encoded `Row` directly (DESIGN
/// §10.2): the primary key, then the other columns in declaration order.
/// Implemented by `#[table]` rows and by bindings' rows of other modules.
pub trait RowCodec: Sized {
    fn encode_row(&self, out: &mut Vec<u8>);
    fn decode_row(input: &mut &[u8]) -> Result<Self, interstice_abi::IntersticeAbiError>;
}

/// Helpers for the generated `RowCodec` impls.
#[doc(hidden)]
pub mod row_wire {
    use interstice_abi::{IntersticeAbiError, IntersticeValue, ValueCodec, wire};

    /// The column count, which must be at least `known`.
    pub fn take_columns(input: &mut &[u8], known: usize, table: &str) -> Result<usize, IntersticeAbiError> {
        let count = wire::take_len(input)?;
        if count < known {
            return Err(wire::error(format_args!("{table}: a row has {count} columns, expected {known}")));
        }
        Ok(count)
    }

    /// Skip the columns a newer version of the table appended.
    pub fn skip_columns(input: &mut &[u8], extra: usize) -> Result<(), IntersticeAbiError> {
        for _ in 0..extra {
            IntersticeValue::decode(input)?;
        }
        Ok(())
    }
}
