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
