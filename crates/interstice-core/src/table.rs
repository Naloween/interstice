use interstice_abi::{
    Row, schema::TableSchema,
};

pub struct Table {
    pub schema: TableSchema,
    pub rows: Vec<Row>,
}
