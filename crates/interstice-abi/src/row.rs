use crate::IntersticeValue;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Row {
    pub primary_key: IntersticeValue,
    pub entries: Vec<IntersticeValue>,
}
impl Into<IntersticeValue> for Row {
    fn into(mut self) -> IntersticeValue {
        let mut all_entries = vec![self.primary_key];
        all_entries.append(&mut self.entries);
        IntersticeValue::Vec(all_entries)
    }
}

impl Into<Row> for IntersticeValue {
    fn into(self) -> Row {
        match self {
            IntersticeValue::Vec(mut vec) => {
                if vec.len() >= 1 {
                    let pk = vec.remove(0);
                    Row {
                        primary_key: pk,
                        entries: vec,
                    }
                } else {
                    panic!("Couldn't convert interstice value to row (empty vec)");
                }
            }
            _ => panic!("Couldn't convert interstice value to row (got {:?})", self),
        }
    }
}
