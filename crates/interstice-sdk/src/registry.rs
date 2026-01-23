use std::sync::{Arc, Mutex};

use interstice_abi::{
    ReducerSchema,
    schema::{SubscriptionSchema, TableSchema},
};
use interstice_sdk_core::REDUCER_REGISTRY;

pub fn collect_tables() -> Vec<TableSchema> {
    vec![]
}

pub fn collect_reducers() -> Vec<ReducerSchema> {
    REDUCER_REGISTRY
        .lock()
        .unwrap()
        .iter()
        .map(|f| f())
        .collect()
}

pub fn collect_subscriptions() -> Vec<SubscriptionSchema> {
    vec![]
}
