use interstice_abi::{IntersticeValue, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum NetworkPacket {
    Handshake {
        node_id: String,
        address: String,
    },
    Close,
    ReducerCall {
        module_name: String,
        reducer_name: String,
        input: IntersticeValue,
    },
    QueryCall {
        request_id: String,
        module_name: String,
        query_name: String,
        input: IntersticeValue,
    },
    QueryResponse {
        request_id: String,
        result: IntersticeValue,
    },
    RequestSubscription(RequestSubscription),
    TableEvent(TableEventInstance),
    ModuleEvent(ModuleEventInstance),
    Error(String),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
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
pub enum TableEventInstance {
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

#[derive(Debug, Serialize, Deserialize)]
pub enum ModuleEventInstance {
    Publish { wasm_binary: Vec<u8> },
    Remove { module_name: String },
}
