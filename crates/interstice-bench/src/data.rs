//! Schemas and generated rows, after SpacetimeDB's `crates/bench`.

use interstice_core::interstice_abi::IntersticeValue;

/// The two row shapes of the generic matrix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Schema {
    /// `{ id: u32, age: u64, name: String }`
    U32U64Str,
    /// `{ id: u32, x: u64, y: u64 }`
    U32U64U64,
}

/// How a table is indexed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexStrategy {
    /// The id is unique (the primary key).
    Unique0,
    /// A non-unique btree on every column; rows keyed by a hidden id.
    BtreeEachColumn,
}

/// One table of the matrix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableId {
    pub schema: Schema,
    pub index: IndexStrategy,
    /// Logged (written to disk) rather than ephemeral.
    pub disk: bool,
}

impl Schema {
    pub fn label(self) -> &'static str {
        match self {
            Schema::U32U64Str => "u32_u64_str",
            Schema::U32U64U64 => "u32_u64_u64",
        }
    }
}

impl IndexStrategy {
    pub fn label(self) -> &'static str {
        match self {
            IndexStrategy::Unique0 => "unique_0",
            IndexStrategy::BtreeEachColumn => "btree_each_column",
        }
    }
}

impl TableId {
    /// The suffix of the bench module's names: `mem_u32_u64_str_unique`.
    pub fn suffix(self) -> String {
        format!(
            "{}_{}_{}",
            if self.disk { "disk" } else { "mem" },
            self.schema.label(),
            match self.index {
                IndexStrategy::Unique0 => "unique",
                IndexStrategy::BtreeEachColumn => "btree",
            }
        )
    }

    /// The row type's name in the bench module: `MemU32U64StrUnique`.
    pub fn type_name(self) -> String {
        format!(
            "{}{}{}",
            if self.disk { "Disk" } else { "Mem" },
            match self.schema {
                Schema::U32U64Str => "U32U64Str",
                Schema::U32U64U64 => "U32U64U64",
            },
            match self.index {
                IndexStrategy::Unique0 => "Unique",
                IndexStrategy::BtreeEachColumn => "Btree",
            }
        )
    }

    /// The table's name in the bench module (the type name, lower-cased).
    pub fn table_name(self) -> String {
        self.type_name().to_lowercase()
    }
}

/// A generated row. `c_str` is set for `U32U64Str`, `c_u64` otherwise.
#[derive(Clone, Debug)]
pub struct BenchRow {
    pub id: u32,
    pub b: u64,
    pub c_str: String,
    pub c_u64: u64,
}

/// SpacetimeDB's generator: xorshift64 from 0xdeadbeef.
pub struct XorShift(u64);

impl XorShift {
    pub fn new() -> Self {
        Self(0xdead_beef)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

impl Default for XorShift {
    fn default() -> Self {
        Self::new()
    }
}

/// `count` rows with ids `start..start+count`; the other columns are drawn
/// as `rng % buckets`, so filtering on one value matches ~count/buckets rows.
pub fn rows(rng: &mut XorShift, start: u32, count: u32, buckets: u64) -> Vec<BenchRow> {
    (start..start + count)
        .map(|id| {
            let b = rng.next_u64() % buckets;
            let c = rng.next_u64() % buckets;
            BenchRow { id, b, c_str: format!("name-{c}"), c_u64: c }
        })
        .collect()
}

/// The filter value matching bucket `bucket` of column 2.
pub fn filter_value(schema: Schema, bucket: u64) -> IntersticeValue {
    match schema {
        Schema::U32U64Str => IntersticeValue::String(format!("name-{bucket}")),
        Schema::U32U64U64 => IntersticeValue::U64(bucket),
    }
}

/// Column names after `id`, per schema.
pub fn columns(schema: Schema) -> (&'static str, &'static str) {
    match schema {
        Schema::U32U64Str => ("age", "name"),
        Schema::U32U64U64 => ("x", "y"),
    }
}

/// The third column's value.
pub fn column_c(schema: Schema, row: &BenchRow) -> IntersticeValue {
    match schema {
        Schema::U32U64Str => IntersticeValue::String(row.c_str.clone()),
        Schema::U32U64U64 => IntersticeValue::U64(row.c_u64),
    }
}

/// `row` as the bench module's struct value (the hidden id is 0, assigned
/// by auto-increment).
pub fn to_value(table: TableId, row: &BenchRow) -> IntersticeValue {
    let mut fields = Vec::with_capacity(4);
    if table.index == IndexStrategy::BtreeEachColumn {
        fields.push(IntersticeValue::U64(0));
    }
    fields.push(IntersticeValue::U32(row.id));
    fields.push(IntersticeValue::U64(row.b));
    fields.push(column_c(table.schema, row));
    IntersticeValue::Struct(fields)
}
