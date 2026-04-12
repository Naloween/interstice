use serde::{Deserialize, Serialize};

use crate::{FieldDef, IntersticeType};
use super::reducer::ReducerTableRef;

/// Declared read access for a query (same [`ReducerTableRef`] rules as reducer `reads`).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QuerySchema {
    pub name: String,
    pub arguments: Vec<FieldDef>,
    pub return_type: IntersticeType,
    #[serde(default)]
    pub reads: Vec<ReducerTableRef>,
}

impl QuerySchema {
    pub fn new(
        name: impl Into<String>,
        arguments: Vec<FieldDef>,
        return_type: IntersticeType,
    ) -> Self {
        Self::with_reads(name, arguments, return_type, Vec::new())
    }

    pub fn with_reads(
        name: impl Into<String>,
        arguments: Vec<FieldDef>,
        return_type: IntersticeType,
        reads: Vec<ReducerTableRef>,
    ) -> Self {
        Self {
            name: name.into(),
            arguments,
            return_type,
            reads,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::QuerySchema;
    use crate::{IntersticeType, ModuleSelection, NodeSelection, ReducerTableRef, decode, encode};

    #[test]
    fn query_schema_round_trip_includes_reads() {
        let schema = QuerySchema::with_reads(
            "total_committed",
            vec![],
            IntersticeType::U64,
            vec![ReducerTableRef {
                node_selection: NodeSelection::Current,
                module_selection: ModuleSelection::Current,
                table_name: "benchprogress".into(),
            }],
        );
        let bytes = encode(&schema).expect("encode");
        let decoded: QuerySchema = decode(&bytes).expect("decode");
        assert_eq!(decoded.name, "total_committed");
        assert_eq!(decoded.reads.len(), 1);
        assert_eq!(decoded.reads[0].table_name, "benchprogress");
    }

    #[test]
    fn query_schema_new_has_empty_reads() {
        let s = QuerySchema::new("health", vec![], IntersticeType::Void);
        assert!(s.reads.is_empty());
    }
}
