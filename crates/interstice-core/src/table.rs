use interstice_abi::{Row, TableSchema};

pub struct Table {
    pub schema: TableSchema,
    pub rows: Vec<Row>,
}
