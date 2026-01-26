use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub enum TableEvent {
    Insert,
    Update,
    Delete,
}

impl Into<TableEvent> for &str {
    fn into(self) -> TableEvent {
        match self {
            "insert" => TableEvent::Insert,
            "update" => TableEvent::Update,
            "delete" => TableEvent::Delete,
            _ => panic!("Couldn't convert string to table event (got {})", self),
        }
    }
}

impl Into<TableEvent> for String {
    fn into(self) -> TableEvent {
        return self.as_str().into();
    }
}

impl Into<&str> for TableEvent {
    fn into(self) -> &'static str {
        match self {
            TableEvent::Insert => "insert",
            TableEvent::Update => "update",
            TableEvent::Delete => "delete",
        }
    }
}
