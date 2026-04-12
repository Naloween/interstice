use serde::{Deserialize, Serialize};

use crate::interstice_type_def::FieldDef;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReducerSchema {
    pub name: String,
    pub arguments: Vec<FieldDef>,
    pub reads: Vec<String>,
    pub inserts: Vec<String>,
    pub updates: Vec<String>,
    pub deletes: Vec<String>,
}

impl ReducerSchema {
    pub fn new(
        name: impl Into<String>,
        arguments: Vec<FieldDef>,
        reads: Vec<String>,
        inserts: Vec<String>,
        updates: Vec<String>,
        deletes: Vec<String>,
    ) -> Self {
        Self {
            name: name.into(),
            arguments,
            reads,
            inserts,
            updates,
            deletes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ReducerSchema;
    use crate::{FieldDef, IntersticeType, decode, encode};

    #[test]
    fn reducer_schema_round_trip_preserves_access_lists() {
        let schema = ReducerSchema::new(
            "hello",
            vec![FieldDef {
                name: "name".to_string(),
                field_type: IntersticeType::String,
            }],
            vec!["users".to_string()],
            vec!["greetings".to_string()],
            vec!["greetings".to_string()],
            vec!["sessions".to_string()],
        );

        let bytes = encode(&schema).expect("encode reducer schema");
        let decoded: ReducerSchema = decode(&bytes).expect("decode reducer schema");

        assert_eq!(decoded.name, "hello");
        assert_eq!(decoded.reads, vec!["users".to_string()]);
        assert_eq!(decoded.inserts, vec!["greetings".to_string()]);
        assert_eq!(decoded.updates, vec!["greetings".to_string()]);
        assert_eq!(decoded.deletes, vec!["sessions".to_string()]);
    }
}
