use interstice_abi::Row;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum NetworkPacket {
    Handshake {
        node_id: String,
        address: String,
    },
    ReducerCall {
        module_name: String,
        reducer_name: String,
    },
    RequestSubscription(RequestSubscription),
    SubscriptionEvent(SubscriptionEvent),
    Error(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RequestSubscription {
    pub module_name: String,
    pub table_name: String,
    pub event: TableEvent,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum TableEvent {
    Insert,
    Update,
    Delete,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum SubscriptionEvent {
    TableInsertEvent {
        module_name: String,
        table_name: String,
        inserted_row: Row,
    },
    TableUpdateEvent {
        module_name: String,
        table_name: String,
        old_row: Row,
        new_row: Row,
    },
    TableDeleteEvent {
        module_name: String,
        table_name: String,
        deleted_row: Row,
    },
}
