//! Capability markers (`ReadFoo`, …) from `#[table]` and tuple composition for context generics.
//!
//! Spelling capabilities out, three ways (most precise first):
//!
//! * `where Caps: CanRead<A> + CanUpdate<B>` on a reducer: the macro reads the bounds.
//!   Shorthands there: [`CanAll<Row>`] (read + insert + update + delete) and tuples of rows,
//!   `CanRead<(A, B, C)>` / `CanAll<(A, B)>`.
//! * [`caps!`](crate::caps!) names a concrete capability set once
//!   (`caps! { pub struct AppCaps { all: [A, B], read: [C] } }`) for reducers taking
//!   `ReducerContext<AppCaps>` and helpers taking `&ReducerContext<AppCaps>`.
//! * A hand-written struct implementing the `Can*` markers and [`ReducerCaps`].
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

/// Full access to `Row`: read, insert, update and delete. Usable both as a reducer
/// `where` bound (the macro expands it) and as a helper bound (it implies the four).
pub trait CanAll<Row>: CanRead<Row> + CanInsert<Row> + CanUpdate<Row> + CanDelete<Row> {}
impl<Row, C: CanRead<Row> + CanInsert<Row> + CanUpdate<Row> + CanDelete<Row>> CanAll<Row> for C {}

#[doc(hidden)]
pub use interstice_abi::ReducerTableRef as __ReducerTableRef;

/// Declare a named, concrete capability set.
///
/// ```ignore
/// caps! {
///     /// Everything the browser touches.
///     pub struct BrowserCaps {
///         all: [Session, Tab, Document],   // read + insert + update + delete
///         read: [MouseState],
///         write: [Log],                    // insert + update + delete
///         insert: [], update: [], delete: [],
///     }
/// }
///
/// #[reducer(on = "load")]
/// fn on_load(ctx: ReducerContext<BrowserCaps>) { … }
/// ```
///
/// Every kind is optional; a row may appear in several kinds as long as no
/// single access (e.g. read) is granted twice.
#[macro_export]
macro_rules! caps {
    ($(#[$meta:meta])* $vis:vis struct $name:ident { $($kind:ident : [$($row:ty),* $(,)?]),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Copy, Clone, Default)]
        $vis struct $name {}
        $($( $crate::caps!(@impl $kind $name $row); )*)*
        impl $crate::ReducerCaps for $name {
            fn extend_reducer_schema(
                reads: &mut Vec<$crate::caps::__ReducerTableRef>,
                inserts: &mut Vec<$crate::caps::__ReducerTableRef>,
                updates: &mut Vec<$crate::caps::__ReducerTableRef>,
                deletes: &mut Vec<$crate::caps::__ReducerTableRef>,
            ) {
                let _ = (&reads, &inserts, &updates, &deletes);
                $($( $crate::caps!(@schema $kind $row, reads, inserts, updates, deletes); )*)*
            }
        }
        impl $crate::QueryCaps for $name {
            fn extend_query_schema(reads: &mut Vec<$crate::caps::__ReducerTableRef>) {
                let (inserts, updates, deletes) = (&mut Vec::new(), &mut Vec::new(), &mut Vec::new());
                let _ = (&reads, &inserts, &updates, &deletes);
                $($( $crate::caps!(@schema $kind $row, reads, inserts, updates, deletes); )*)*
            }
        }
    };

    (@impl all $name:ident $row:ty) => {
        impl $crate::CanRead<$row> for $name {}
        impl $crate::CanInsert<$row> for $name {}
        impl $crate::CanUpdate<$row> for $name {}
        impl $crate::CanDelete<$row> for $name {}
    };
    (@impl write $name:ident $row:ty) => {
        impl $crate::CanInsert<$row> for $name {}
        impl $crate::CanUpdate<$row> for $name {}
        impl $crate::CanDelete<$row> for $name {}
    };
    (@impl read $name:ident $row:ty) => { impl $crate::CanRead<$row> for $name {} };
    (@impl insert $name:ident $row:ty) => { impl $crate::CanInsert<$row> for $name {} };
    (@impl update $name:ident $row:ty) => { impl $crate::CanUpdate<$row> for $name {} };
    (@impl delete $name:ident $row:ty) => { impl $crate::CanDelete<$row> for $name {} };
    (@impl $other:ident $name:ident $row:ty) => {
        compile_error!(concat!("unknown capability kind `", stringify!($other), "` (expected all, read, write, insert, update or delete)"));
    };

    (@schema all $row:ty, $r:ident, $i:ident, $u:ident, $d:ident) => {
        $r.push(<$row as $crate::TableRow>::table_ref());
        $i.push(<$row as $crate::TableRow>::table_ref());
        $u.push(<$row as $crate::TableRow>::table_ref());
        $d.push(<$row as $crate::TableRow>::table_ref());
    };
    (@schema write $row:ty, $r:ident, $i:ident, $u:ident, $d:ident) => {
        $i.push(<$row as $crate::TableRow>::table_ref());
        $u.push(<$row as $crate::TableRow>::table_ref());
        $d.push(<$row as $crate::TableRow>::table_ref());
    };
    (@schema read $row:ty, $r:ident, $i:ident, $u:ident, $d:ident) => { $r.push(<$row as $crate::TableRow>::table_ref()); };
    (@schema insert $row:ty, $r:ident, $i:ident, $u:ident, $d:ident) => { $i.push(<$row as $crate::TableRow>::table_ref()); };
    (@schema update $row:ty, $r:ident, $i:ident, $u:ident, $d:ident) => { $u.push(<$row as $crate::TableRow>::table_ref()); };
    (@schema delete $row:ty, $r:ident, $i:ident, $u:ident, $d:ident) => { $d.push(<$row as $crate::TableRow>::table_ref()); };
    (@schema $other:ident $row:ty, $r:ident, $i:ident, $u:ident, $d:ident) => {};
}

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
