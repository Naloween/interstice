use interstice_abi::NodeSchema;
use interstice_abi::{IntersticeValue, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum NetworkPacket {
    /// Sent inside the TLS channel; `node_id` must be the key's (DESIGN §7.4).
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
    RequestUnsubscription(RequestSubscription),
    RequestTableSync {
        module_name: String,
        table_name: String,
    },
    TableSyncResponse {
        module_name: String,
        table_name: String,
        rows: Vec<Row>,
    },
    TableEvent(TableEventInstance),
    ModuleEvent(ModuleEventInstance),
    SchemaRequest {
        request_id: String,
        node_name: String,
    },
    SchemaResponse {
        request_id: String,
        schema: NodeSchema,
    },
    Error(String),
    /// The answer to a `ModuleEvent::Load` (DESIGN §3.3).
    ModuleLoadResult {
        request_id: String,
        outcome: LoadOutcome,
    },
}

/// What happened to a module load.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum LoadOutcome {
    /// Loaded; the plan that was applied (rendered).
    Loaded { plan: String },
    /// Nothing changed: the plan needs a decision (rendered).
    NeedsDecision { plan: String },
    /// Nothing changed: the module asks for authorities not granted yet.
    NeedsGrant { module: String, authorities: Vec<String> },
    Failed(String),
    /// Handed to the module holding the Module authority.
    Forwarded,
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
    Load {
        wasm_binary: Vec<u8>,
        policy: crate::runtime::migration::MigrationPolicy,
        /// Answered with `ModuleLoadResult` when not empty.
        request_id: String,
        /// The requester grants the authorities the module asks for
        /// (DESIGN §7.4); otherwise new ones are answered `NeedsGrant`.
        grant: bool,
    },
    Unload { module_name: String },
    Remove { module_name: String },
}
