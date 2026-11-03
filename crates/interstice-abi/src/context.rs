use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RawReducerContext {
    pub caller_node_id: String,
    /// Schema name of the module that invoked this reducer, when the call came
    /// from another module on the same node. Empty for runtime-originated calls
    /// (events, render, CLI, network). Unlike `caller_node_id` (shared by every
    /// module on a node), this distinguishes co-located callers.
    pub caller_module_name: String,
    /// Per-module counter bumped by the host whenever a call of this module
    /// produced effects that were *not* committed (rejected commit). Anything
    /// a module instance derived from its own calls (instance caches) must be
    /// discarded when this changes. See `interstice_sdk::cache`.
    pub state_epoch: u64,
}

impl RawReducerContext {
    pub fn new(caller_node_id: String, caller_module_name: String, state_epoch: u64) -> Self {
        Self {
            caller_node_id,
            caller_module_name,
            state_epoch,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RawQueryContext {
    pub caller_node_id: String,
    /// See [`RawReducerContext::caller_module_name`].
    pub caller_module_name: String,
    /// See [`RawReducerContext::state_epoch`].
    pub state_epoch: u64,
}

impl RawQueryContext {
    pub fn new(caller_node_id: String, caller_module_name: String, state_epoch: u64) -> Self {
        Self {
            caller_node_id,
            caller_module_name,
            state_epoch,
        }
    }
}
