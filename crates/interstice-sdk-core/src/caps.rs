//! Capability markers (`ReadFoo`, …) from `#[table]` and tuple composition for context generics.
//!
//! [`ReducerCaps`] / [`QueryCaps`] compose for nested pairs `(A, B)` and recurse for schema.
//! [`CanRead`] / [`CanInsert`] / [`CanUpdate`] / [`CanDelete`] do **not** infer through `(A, B)` (Rust
//! cannot express “either side” without overlapping impls). For multi-table reducer caps, use a
//! module-local zero-sized struct and implement the needed `Can*` markers per row type (often via a
//! small declarative macro in your crate), delegating [`ReducerCaps::extend_reducer_schema`] to a
//! nested tuple of `Read*` / `Insert*` / … markers.

use interstice_abi::ReducerTableRef;

/// Read access to rows of type `Row` (the `#[table]` struct).
pub trait CanRead<Row> {}
/// Insert access for `Row`.
pub trait CanInsert<Row> {}
/// Update access for `Row`.
pub trait CanUpdate<Row> {}
/// Delete access for `Row`.
pub trait CanDelete<Row> {}

/// One leaf of a reducer `Caps` tree: contributes [`ReducerTableRef`] entries.
pub trait ReducerCapPiece {
    fn extend_reducer_schema(
        reads: &mut Vec<ReducerTableRef>,
        inserts: &mut Vec<ReducerTableRef>,
        updates: &mut Vec<ReducerTableRef>,
        deletes: &mut Vec<ReducerTableRef>,
    );
}

/// Composed reducer capabilities (markers, tuples, or `()`).
pub trait ReducerCaps {
    fn extend_reducer_schema(
        reads: &mut Vec<ReducerTableRef>,
        inserts: &mut Vec<ReducerTableRef>,
        updates: &mut Vec<ReducerTableRef>,
        deletes: &mut Vec<ReducerTableRef>,
    );
}

impl ReducerCaps for () {
    fn extend_reducer_schema(
        _reads: &mut Vec<ReducerTableRef>,
        _inserts: &mut Vec<ReducerTableRef>,
        _updates: &mut Vec<ReducerTableRef>,
        _deletes: &mut Vec<ReducerTableRef>,
    ) {
    }
}

impl<T: ReducerCapPiece> ReducerCaps for T {
    fn extend_reducer_schema(
        reads: &mut Vec<ReducerTableRef>,
        inserts: &mut Vec<ReducerTableRef>,
        updates: &mut Vec<ReducerTableRef>,
        deletes: &mut Vec<ReducerTableRef>,
    ) {
        T::extend_reducer_schema(reads, inserts, updates, deletes);
    }
}

impl<A: ReducerCaps, B: ReducerCaps> ReducerCaps for (A, B) {
    fn extend_reducer_schema(
        reads: &mut Vec<ReducerTableRef>,
        inserts: &mut Vec<ReducerTableRef>,
        updates: &mut Vec<ReducerTableRef>,
        deletes: &mut Vec<ReducerTableRef>,
    ) {
        A::extend_reducer_schema(reads, inserts, updates, deletes);
        B::extend_reducer_schema(reads, inserts, updates, deletes);
    }
}

impl<A: ReducerCaps> ReducerCaps for (A,) {
    fn extend_reducer_schema(
        reads: &mut Vec<ReducerTableRef>,
        inserts: &mut Vec<ReducerTableRef>,
        updates: &mut Vec<ReducerTableRef>,
        deletes: &mut Vec<ReducerTableRef>,
    ) {
        A::extend_reducer_schema(reads, inserts, updates, deletes);
    }
}

/// One leaf of a query `Caps` tree.
pub trait QueryCapPiece {
    fn extend_query_schema(reads: &mut Vec<ReducerTableRef>);
}

pub trait QueryCaps {
    fn extend_query_schema(reads: &mut Vec<ReducerTableRef>);
}

impl QueryCaps for () {
    fn extend_query_schema(_reads: &mut Vec<ReducerTableRef>) {}
}

impl<T: QueryCapPiece> QueryCaps for T {
    fn extend_query_schema(reads: &mut Vec<ReducerTableRef>) {
        T::extend_query_schema(reads);
    }
}

impl<A: QueryCaps, B: QueryCaps> QueryCaps for (A, B) {
    fn extend_query_schema(reads: &mut Vec<ReducerTableRef>) {
        A::extend_query_schema(reads);
        B::extend_query_schema(reads);
    }
}

impl<A: QueryCaps> QueryCaps for (A,) {
    fn extend_query_schema(reads: &mut Vec<ReducerTableRef>) {
        A::extend_query_schema(reads);
    }
}

/// Single-field tuple wrapper delegates to the inner marker (see module-level docs for composition).
macro_rules! impl_disjunctive_can {
    ($trait:ident) => {
        impl<Row, A> $trait<Row> for (A,) where A: $trait<Row> {}
    };
}

impl_disjunctive_can!(CanRead);
impl_disjunctive_can!(CanInsert);
impl_disjunctive_can!(CanUpdate);
impl_disjunctive_can!(CanDelete);
