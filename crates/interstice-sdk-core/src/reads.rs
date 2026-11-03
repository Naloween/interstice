//! Typed table reads used by `#[table]` and binding codegen.
//!
//! Every generated read (`scan`, `get`, `scan_by_*`, `get_by_*`) returns
//! `Result<_, String>` instead of panicking: a missing table, a denied
//! capability or a row that fails to decode surfaces as `Err`. Reducers may
//! return `Result<(), E>` and propagate with `?`; an `Err` aborts the reducer
//! (its transaction is discarded), exactly like a panic but with a clean message.

use interstice_abi::{IndexKey, IndexQuery, IntersticeValue, ModuleSelection, Row};

/// Convert any key-like value into an [`IndexKey`].
pub fn to_index_key<K: Into<IntersticeValue>>(value: K) -> Result<IndexKey, String> {
    TryInto::<IndexKey>::try_into(Into::<IntersticeValue>::into(value))
        .map_err(|e| format!("value is not a valid index key: {e}"))
}

fn decode_rows<T>(table: &str, rows: Vec<Row>) -> Result<Vec<T>, String>
where
    T: TryFrom<Row>,
    T::Error: std::fmt::Display,
{
    rows.into_iter()
        .map(|row| T::try_from(row).map_err(|e| format!("{table}: row decode failed: {e}")))
        .collect()
}

/// All rows of `table`.
pub fn read_scan<T>(module: ModuleSelection, table: &str) -> Result<Vec<T>, String>
where
    T: TryFrom<Row>,
    T::Error: std::fmt::Display,
{
    let rows = crate::host_calls::scan(module, table.to_string())
        .map_err(|e| format!("{table}: scan failed: {e}"))?;
    decode_rows(table, rows)
}

/// The row of `table` with primary key `key`, if any.
pub fn read_get<T, K>(module: ModuleSelection, table: &str, key: K) -> Result<Option<T>, String>
where
    T: TryFrom<Row>,
    T::Error: std::fmt::Display,
    K: Into<IntersticeValue>,
{
    let row = crate::host_calls::get_by_primary_key(module, table, to_index_key(key)?)
        .map_err(|e| format!("{table}: get failed: {e}"))?;
    row.map(|r| T::try_from(r).map_err(|e| format!("{table}: row decode failed: {e}")))
        .transpose()
}

/// Rows of `table` matching `query` on `index`.
pub fn read_index<T>(
    module: ModuleSelection,
    table: &str,
    index: &str,
    query: IndexQuery,
) -> Result<Vec<T>, String>
where
    T: TryFrom<Row>,
    T::Error: std::fmt::Display,
{
    let rows =
        crate::host_calls::scan_index(module, table.to_string(), index.to_string(), query)
            .map_err(|e| format!("{table}.{index}: index scan failed: {e}"))?;
    decode_rows(table, rows)
}

/// What a reducer body may return: `()` or `Result<(), E: Display>`.
/// `Err` aborts the reducer with the error message.
pub trait ReducerOutcome {
    fn into_outcome(self) -> Result<(), String>;
}

impl ReducerOutcome for () {
    fn into_outcome(self) -> Result<(), String> {
        Ok(())
    }
}

impl<E: std::fmt::Display> ReducerOutcome for Result<(), E> {
    fn into_outcome(self) -> Result<(), String> {
        self.map_err(|e| e.to_string())
    }
}
