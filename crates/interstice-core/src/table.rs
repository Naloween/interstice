use std::collections::HashMap;

use interstice_abi::{
    Row, interstice_type_def::IntersticeTypeDef, schema::TableSchema, validate_value,
};

pub struct Table {
    pub schema: TableSchema,
    pub rows: Vec<Row>,
}
