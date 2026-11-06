//! The workloads of `interstice-bench` (see `benchmarks/README.md`).
//!
//! Two families, after SpacetimeDB's own suites so the numbers compare:
//! - the generic matrix of `crates/bench`: schemas `u32_u64_str` and
//!   `u32_u64_u64`, index strategies `unique_0` (unique id) and
//!   `btree_each_column` (a non-unique btree on every column), in memory
//!   (ephemeral) and on disk (logged); operations empty, insert_bulk,
//!   update_bulk, iterate and filter, one transaction per call;
//! - the keynote transfer: `transfer(from, to, amount)` between accounts.

use interstice_sdk::*;

interstice_module!(visibility: Public);

/// One table of the generic matrix and its reducers.
///
/// `unique_0`: the id is the primary key. `btree_each_column`: rows are
/// keyed by a hidden auto-increment id (the engine needs a primary key) and
/// every column has a non-unique btree, as in SpacetimeDB.
macro_rules! unique_0 {
    ($row:ident, $table:ident, $persist:ident, $b:ident: $bt:ty, $c:ident: $ct:ty,
     $insert:ident, $update:ident, $iterate:ident, $clear:ident) => {
        #[table(public, $persist)]
        pub struct $row {
            #[primary_key]
            pub id: u32,
            pub $b: $bt,
            pub $c: $ct,
        }

        #[reducer]
        pub fn $insert<Caps: CanInsert<$row>>(ctx: ReducerContext<Caps>, rows: Vec<$row>) -> Result<(), String> {
            for row in rows {
                ctx.current.tables.$table().insert(row)?;
            }
            Ok(())
        }

        /// Bump the second column of the first `count` rows, by id.
        #[reducer]
        pub fn $update<Caps>(ctx: ReducerContext<Caps>, count: u32) -> Result<(), String>
        where
            Caps: CanRead<$row> + CanUpdate<$row>,
        {
            let rows = ctx.current.tables.$table().scan()?;
            for mut row in rows.into_iter().take(count as usize) {
                row.$b += 1;
                ctx.current.tables.$table().update(row)?;
            }
            Ok(())
        }

        #[reducer]
        pub fn $iterate<Caps: CanRead<$row>>(ctx: ReducerContext<Caps>) -> Result<(), String> {
            let mut sum = 0u64;
            for row in ctx.current.tables.$table().scan()? {
                sum = sum.wrapping_add(row.id as u64);
            }
            core::hint::black_box(sum);
            Ok(())
        }

        #[reducer]
        pub fn $clear<Caps: CanDelete<$row>>(ctx: ReducerContext<Caps>) -> Result<(), String> {
            ctx.current.tables.$table().clear()?;
            Ok(())
        }
    };
}

macro_rules! btree_each_column {
    ($row:ident, $table:ident, $persist:ident, $b:ident: $bt:ty, $c:ident: $ct:ty, $filter_ty:ty,
     $insert:ident, $filter:ident, $scan_eq:ident, $clear:ident) => {
        #[table(public, $persist)]
        pub struct $row {
            #[primary_key(auto_inc)]
            pub rid: u64,
            #[index(btree)]
            pub id: u32,
            #[index(btree)]
            pub $b: $bt,
            #[index(btree)]
            pub $c: $ct,
        }

        #[reducer]
        pub fn $insert<Caps: CanInsert<$row>>(ctx: ReducerContext<Caps>, rows: Vec<$row>) -> Result<(), String> {
            for row in rows {
                ctx.current.tables.$table().insert(row)?;
            }
            Ok(())
        }

        /// Rows whose third column equals `value`, through its index.
        #[reducer]
        pub fn $filter<Caps: CanRead<$row>>(ctx: ReducerContext<Caps>, value: $filter_ty) -> Result<(), String> {
            let rows = ctx.current.tables.$table().$scan_eq(value)?;
            core::hint::black_box(rows.len());
            Ok(())
        }

        #[reducer]
        pub fn $clear<Caps: CanDelete<$row>>(ctx: ReducerContext<Caps>) -> Result<(), String> {
            ctx.current.tables.$table().clear()?;
            Ok(())
        }
    };
}

unique_0!(MemU32U64StrUnique, memu32u64strunique, ephemeral, age: u64, name: String,
    insert_bulk_mem_u32_u64_str_unique, update_bulk_mem_u32_u64_str_unique,
    iterate_mem_u32_u64_str_unique, clear_mem_u32_u64_str_unique);
unique_0!(MemU32U64U64Unique, memu32u64u64unique, ephemeral, x: u64, y: u64,
    insert_bulk_mem_u32_u64_u64_unique, update_bulk_mem_u32_u64_u64_unique,
    iterate_mem_u32_u64_u64_unique, clear_mem_u32_u64_u64_unique);
unique_0!(DiskU32U64StrUnique, disku32u64strunique, logged, age: u64, name: String,
    insert_bulk_disk_u32_u64_str_unique, update_bulk_disk_u32_u64_str_unique,
    iterate_disk_u32_u64_str_unique, clear_disk_u32_u64_str_unique);
unique_0!(DiskU32U64U64Unique, disku32u64u64unique, logged, x: u64, y: u64,
    insert_bulk_disk_u32_u64_u64_unique, update_bulk_disk_u32_u64_u64_unique,
    iterate_disk_u32_u64_u64_unique, clear_disk_u32_u64_u64_unique);

btree_each_column!(MemU32U64StrBtree, memu32u64strbtree, ephemeral, age: u64, name: String, String,
    insert_bulk_mem_u32_u64_str_btree, filter_mem_u32_u64_str_btree, scan_by_name_eq,
    clear_mem_u32_u64_str_btree);
btree_each_column!(MemU32U64U64Btree, memu32u64u64btree, ephemeral, x: u64, y: u64, u64,
    insert_bulk_mem_u32_u64_u64_btree, filter_mem_u32_u64_u64_btree, scan_by_y_eq,
    clear_mem_u32_u64_u64_btree);
btree_each_column!(DiskU32U64StrBtree, disku32u64strbtree, logged, age: u64, name: String, String,
    insert_bulk_disk_u32_u64_str_btree, filter_disk_u32_u64_str_btree, scan_by_name_eq,
    clear_disk_u32_u64_str_btree);
btree_each_column!(DiskU32U64U64Btree, disku32u64u64btree, logged, x: u64, y: u64, u64,
    insert_bulk_disk_u32_u64_u64_btree, filter_disk_u32_u64_u64_btree, scan_by_y_eq,
    clear_disk_u32_u64_u64_btree);

/// The empty transaction: the fixed cost of one reducer call.
#[reducer]
pub fn empty(_ctx: ReducerContext) {}

// ---- Keynote transfer (TPC-B-like) ------------------------------------------

#[table(public, logged)]
pub struct Account {
    #[primary_key]
    pub id: u32,
    pub balance: i64,
}

/// Create accounts `0..count` with `balance` each, replacing any.
#[reducer]
pub fn seed_accounts<Caps: CanAll<Account>>(ctx: ReducerContext<Caps>, count: u32, balance: i64) -> Result<(), String> {
    ctx.current.tables.account().clear()?;
    for id in 0..count {
        ctx.current.tables.account().insert(Account { id, balance })?;
    }
    Ok(())
}

/// Move `amount` from one account to another, as SpacetimeDB's keynote.
#[reducer]
pub fn transfer<Caps>(ctx: ReducerContext<Caps>, from: u32, to: u32, amount: i64) -> Result<(), String>
where
    Caps: CanRead<Account> + CanUpdate<Account>,
{
    if from == to || amount <= 0 {
        return Err("invalid transfer".into());
    }
    let accounts = ctx.current.tables.account();
    let mut a = accounts.get(from)?.ok_or("no such account")?;
    let mut b = accounts.get(to)?.ok_or("no such account")?;
    if a.balance < amount {
        return Err("insufficient funds".into());
    }
    a.balance -= amount;
    b.balance += amount;
    accounts.update(a)?;
    accounts.update(b)?;
    Ok(())
}

/// The sum of all balances: constant under transfers.
#[query]
pub fn total_balance<Caps: CanRead<Account>>(ctx: QueryContext<Caps>) -> Result<i64, String> {
    Ok(ctx.current.tables.account().scan()?.iter().map(|a| a.balance).sum())
}
