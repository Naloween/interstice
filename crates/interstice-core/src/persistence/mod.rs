//! Persistence of tables (DESIGN §3.2): logged tables as segmented logs
//! with checkpoints, stateful tables as one file per row.

mod table_store;

pub use table_store::{LogOperation, SnapshotPlan, TableStore};
