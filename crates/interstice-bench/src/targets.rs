//! The three targets of the generic matrix, behind one trait, as in
//! SpacetimeDB's `BenchDatabase`: the raw datastore, the bench module
//! called in process, and SQLite.

use crate::data::{self, BenchRow, IndexStrategy, Schema, TableId};
use crate::node::BenchNode;
use interstice_core::datastore::{Table, TableOverlay};
use interstice_core::interstice_abi::{IndexKey, IndexQuery, IntersticeValue, Row, TableSchema};
use std::collections::HashMap;

pub trait BenchDatabase {
    fn name(&self) -> &'static str;
    /// The fixed cost of one transaction, if the target has one to measure.
    fn empty(&mut self) {}
    fn clear(&mut self, table: TableId);
    fn insert_bulk(&mut self, table: TableId, rows: &[BenchRow]);
    /// Bump column 2 of the first `count` rows (`unique_0` tables).
    fn update_bulk(&mut self, table: TableId, count: u32);
    /// Read every row.
    fn iterate(&mut self, table: TableId);
    /// Rows whose column 3 is in `bucket`, through its index.
    fn filter(&mut self, table: TableId, bucket: u64);
}

// ---- Module ---------------------------------------------------------------

/// The bench module on a headless node; every operation is one reducer
/// call, committed before it returns.
pub struct ModuleTarget {
    pub node: BenchNode,
}

impl ModuleTarget {
    pub fn new() -> Self {
        Self { node: BenchNode::start() }
    }

    fn call(&self, reducer: String, args: Vec<IntersticeValue>) {
        self.node
            .call(&reducer, args)
            .unwrap_or_else(|e| panic!("{reducer}: {e}"));
    }
}

impl Default for ModuleTarget {
    fn default() -> Self {
        Self::new()
    }
}

impl BenchDatabase for ModuleTarget {
    fn name(&self) -> &'static str {
        "module"
    }
    fn empty(&mut self) {
        self.call("empty".into(), vec![]);
    }
    fn clear(&mut self, table: TableId) {
        self.call(format!("clear_{}", table.suffix()), vec![]);
    }
    fn insert_bulk(&mut self, table: TableId, rows: &[BenchRow]) {
        let rows = rows.iter().map(|r| data::to_value(table, r)).collect();
        self.call(format!("insert_bulk_{}", table.suffix()), vec![IntersticeValue::Vec(rows)]);
    }
    fn update_bulk(&mut self, table: TableId, count: u32) {
        self.call(format!("update_bulk_{}", table.suffix()), vec![IntersticeValue::U32(count)]);
    }
    fn iterate(&mut self, table: TableId) {
        self.call(format!("iterate_{}", table.suffix()), vec![]);
    }
    fn filter(&mut self, table: TableId, bucket: u64) {
        self.call(
            format!("filter_{}", table.suffix()),
            vec![data::filter_value(table.schema, bucket)],
        );
    }
}

// ---- Raw ------------------------------------------------------------------

/// The engine's in-memory tables, with the same staging a reducer does:
/// writes are validated into an overlay, then applied. No WASM, no
/// scheduler, no persistence; memory tables only.
pub struct RawTarget {
    schemas: HashMap<String, TableSchema>,
    tables: HashMap<String, Table>,
    next_rid: u64,
}

impl RawTarget {
    pub fn new() -> Self {
        let schema = interstice_core::bundle::describe_wasm(&crate::node::wasm()).expect("describe bench");
        let schemas = schema.tables.into_iter().map(|t| (t.name.clone(), t)).collect();
        Self { schemas, tables: HashMap::new(), next_rid: 1 }
    }

    fn table(&mut self, table: TableId) -> &mut Table {
        let name = table.table_name();
        let schema = self.schemas[&name].clone();
        self.tables.entry(name).or_insert_with(|| Table::new(schema))
    }

    fn row(&mut self, table: TableId, r: &BenchRow) -> Row {
        let c = data::column_c(table.schema, r);
        match table.index {
            IndexStrategy::Unique0 => Row {
                primary_key: IntersticeValue::U32(r.id),
                entries: vec![IntersticeValue::U64(r.b), c],
            },
            IndexStrategy::BtreeEachColumn => {
                let rid = self.next_rid;
                self.next_rid += 1;
                Row {
                    primary_key: IntersticeValue::U64(rid),
                    entries: vec![IntersticeValue::U32(r.id), IntersticeValue::U64(r.b), c],
                }
            }
        }
    }
}

impl Default for RawTarget {
    fn default() -> Self {
        Self::new()
    }
}

fn pk(row: &Row) -> IndexKey {
    IndexKey::try_from(&row.primary_key).expect("index key")
}

impl BenchDatabase for RawTarget {
    fn name(&self) -> &'static str {
        "raw"
    }
    fn clear(&mut self, table: TableId) {
        self.table(table).clear();
    }
    fn insert_bulk(&mut self, table: TableId, rows: &[BenchRow]) {
        let rows: Vec<Row> = rows.iter().map(|r| self.row(table, r)).collect();
        let t = self.table(table);
        let mut ov = TableOverlay::new(t.version());
        for row in &rows {
            t.ov_validate_insert(Some(&ov), row).expect("insert");
            t.ov_record(&mut ov, pk(row), Some(row.clone())).expect("record");
        }
        for row in rows {
            t.insert_trusted(row).expect("apply");
        }
    }
    fn update_bulk(&mut self, table: TableId, count: u32) {
        let t = self.table(table);
        let mut ov = TableOverlay::new(t.version());
        let mut updated = Vec::with_capacity(count as usize);
        for row in t.scan().iter().take(count as usize) {
            let mut row = row.clone();
            if let IntersticeValue::U64(b) = &mut row.entries[0] {
                *b += 1;
            }
            t.ov_validate_update(Some(&ov), &row).expect("update");
            t.ov_record(&mut ov, pk(&row), Some(row.clone())).expect("record");
            updated.push(row);
        }
        for row in updated {
            t.update(row).expect("apply");
        }
    }
    fn iterate(&mut self, table: TableId) {
        let t = self.table(table);
        let mut n = 0u64;
        for row in t.scan() {
            n = n.wrapping_add(std::hint::black_box(row).entries.len() as u64);
        }
        std::hint::black_box(n);
    }
    fn filter(&mut self, table: TableId, bucket: u64) {
        let (_, c) = data::columns(table.schema);
        let key = IndexKey::try_from(&data::filter_value(table.schema, bucket)).expect("key");
        let t = self.table(table);
        let rows = t.get_by_index(c, &IndexQuery::Eq(key)).expect("filter");
        std::hint::black_box(rows.len());
    }
}

// ---- SQLite ---------------------------------------------------------------

/// SQLite through rusqlite, as SpacetimeDB's reference: in memory, or a
/// file in WAL mode with `synchronous = NORMAL`. One transaction per call.
pub struct SqliteTarget {
    conn: rusqlite::Connection,
    _dir: Option<tempfile::TempDir>,
}

impl SqliteTarget {
    pub fn new(disk: bool) -> Self {
        let (conn, dir) = if disk {
            let dir = tempfile::tempdir().expect("temp dir");
            let conn = rusqlite::Connection::open(dir.path().join("bench.db")).expect("sqlite");
            conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")
                .expect("pragmas");
            (conn, Some(dir))
        } else {
            (rusqlite::Connection::open_in_memory().expect("sqlite"), None)
        };
        let mut target = Self { conn, _dir: dir };
        for schema in [Schema::U32U64Str, Schema::U32U64U64] {
            for index in [IndexStrategy::Unique0, IndexStrategy::BtreeEachColumn] {
                target.create(TableId { schema, index, disk });
            }
        }
        target
    }

    fn create(&mut self, table: TableId) {
        let (b, c) = data::columns(table.schema);
        let c_type = if table.schema == Schema::U32U64Str { "TEXT" } else { "INTEGER" };
        let name = table.table_name();
        let sql = match table.index {
            IndexStrategy::Unique0 => format!(
                "CREATE TABLE {name} (id INTEGER PRIMARY KEY, {b} INTEGER NOT NULL, {c} {c_type} NOT NULL);"
            ),
            IndexStrategy::BtreeEachColumn => format!(
                "CREATE TABLE {name} (id INTEGER NOT NULL, {b} INTEGER NOT NULL, {c} {c_type} NOT NULL);
                 CREATE INDEX {name}_id ON {name}(id);
                 CREATE INDEX {name}_{b} ON {name}({b});
                 CREATE INDEX {name}_{c} ON {name}({c});"
            ),
        };
        self.conn.execute_batch(&sql).expect("create table");
    }
}

impl BenchDatabase for SqliteTarget {
    fn name(&self) -> &'static str {
        "sqlite"
    }
    fn clear(&mut self, table: TableId) {
        self.conn
            .execute(&format!("DELETE FROM {}", table.table_name()), [])
            .expect("clear");
    }
    fn insert_bulk(&mut self, table: TableId, rows: &[BenchRow]) {
        let (b, c) = data::columns(table.schema);
        let tx = self.conn.transaction().expect("tx");
        {
            let mut stmt = tx
                .prepare_cached(&format!("INSERT INTO {} (id, {b}, {c}) VALUES (?, ?, ?)", table.table_name()))
                .expect("prepare");
            for r in rows {
                match table.schema {
                    Schema::U32U64Str => stmt.execute(rusqlite::params![r.id, r.b as i64, r.c_str]),
                    Schema::U32U64U64 => stmt.execute(rusqlite::params![r.id, r.b as i64, r.c_u64 as i64]),
                }
                .expect("insert");
            }
        }
        tx.commit().expect("commit");
    }
    fn update_bulk(&mut self, table: TableId, count: u32) {
        let (b, _) = data::columns(table.schema);
        let name = table.table_name();
        let tx = self.conn.transaction().expect("tx");
        {
            let ids: Vec<u32> = tx
                .prepare_cached(&format!("SELECT id FROM {name} LIMIT ?"))
                .expect("prepare")
                .query_map([count], |r| r.get(0))
                .expect("select")
                .collect::<Result<_, _>>()
                .expect("rows");
            let mut stmt = tx
                .prepare_cached(&format!("UPDATE {name} SET {b} = {b} + 1 WHERE id = ?"))
                .expect("prepare");
            for id in ids {
                stmt.execute([id]).expect("update");
            }
        }
        tx.commit().expect("commit");
    }
    fn iterate(&mut self, table: TableId) {
        let (b, c) = data::columns(table.schema);
        let mut stmt = self
            .conn
            .prepare_cached(&format!("SELECT id, {b}, {c} FROM {}", table.table_name()))
            .expect("prepare");
        let mut rows = stmt.query([]).expect("select");
        let mut n = 0u64;
        while let Some(row) = rows.next().expect("row") {
            n = n.wrapping_add(row.get::<_, u32>(0).expect("id") as u64);
        }
        std::hint::black_box(n);
    }
    fn filter(&mut self, table: TableId, bucket: u64) {
        let (b, c) = data::columns(table.schema);
        let mut stmt = self
            .conn
            .prepare_cached(&format!("SELECT id, {b}, {c} FROM {} WHERE {c} = ?", table.table_name()))
            .expect("prepare");
        let n = match table.schema {
            Schema::U32U64Str => stmt.query([format!("name-{bucket}")]).expect("q").mapped(|_| Ok(())).count(),
            Schema::U32U64U64 => stmt.query([bucket as i64]).expect("q").mapped(|_| Ok(())).count(),
        };
        std::hint::black_box(n);
    }
}
