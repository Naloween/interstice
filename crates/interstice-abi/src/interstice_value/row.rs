use crate::{IntersticeValue, Row};

impl IntersticeValue {
    pub fn from_row(row: &Row) -> Self {
        let mut values = Vec::with_capacity(1 + row.entries.len());
        values.push(row.primary_key.clone());
        values.extend_from_slice(&row.entries);
        IntersticeValue::Vec(values)
    }
}
