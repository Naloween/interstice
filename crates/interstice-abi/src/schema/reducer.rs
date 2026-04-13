use serde::{Deserialize, Serialize};

use crate::interstice_type_def::FieldDef;
use crate::{ModuleSelection, NodeSelection};

/// Declared table access for a reducer: [`NodeSelection`] + [`ModuleSelection`] + `table_name`.
/// Use [`ModuleSelection::Current`] for the module that contains the reducer (no embedded crate name);
/// the runtime matches that against the active call frame / host-call [`ModuleSelection`], like table scans.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ReducerTableRef {
    pub node_selection: NodeSelection,
    pub module_selection: ModuleSelection,
    pub table_name: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReducerSchema {
    pub name: String,
    pub arguments: Vec<FieldDef>,
    pub reads: Vec<ReducerTableRef>,
    pub inserts: Vec<ReducerTableRef>,
    pub updates: Vec<ReducerTableRef>,
    pub deletes: Vec<ReducerTableRef>,
}

impl ReducerSchema {
    pub fn new(
        name: impl Into<String>,
        arguments: Vec<FieldDef>,
        reads: Vec<ReducerTableRef>,
        inserts: Vec<ReducerTableRef>,
        updates: Vec<ReducerTableRef>,
        deletes: Vec<ReducerTableRef>,
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
    use super::{ReducerSchema, ReducerTableRef};
    use crate::{FieldDef, IntersticeType, ModuleSelection, NodeSelection, decode, encode};

    #[test]
    fn reducer_schema_round_trip_preserves_access_lists() {
        let schema = ReducerSchema::new(
            "hello",
            vec![FieldDef {
                name: "name".to_string(),
                field_type: IntersticeType::String,
            }],
            vec![ReducerTableRef {
                node_selection: NodeSelection::Current,
                module_selection: ModuleSelection::Other("hello-example".to_string()),
                table_name: "users".to_string(),
            }],
            vec![ReducerTableRef {
                node_selection: NodeSelection::Current,
                module_selection: ModuleSelection::Other("hello-example".to_string()),
                table_name: "greetings".to_string(),
            }],
            vec![ReducerTableRef {
                node_selection: NodeSelection::Current,
                module_selection: ModuleSelection::Other("hello-example".to_string()),
                table_name: "greetings".to_string(),
            }],
            vec![ReducerTableRef {
                node_selection: NodeSelection::Current,
                module_selection: ModuleSelection::Other("hello-example".to_string()),
                table_name: "sessions".to_string(),
            }],
        );

        let bytes = encode(&schema).expect("encode reducer schema");
        let decoded: ReducerSchema = decode(&bytes).expect("decode reducer schema");

        assert_eq!(decoded.name, "hello");
        assert_eq!(decoded.reads.len(), 1);
        assert_eq!(decoded.reads[0].table_name, "users");
    }
}
