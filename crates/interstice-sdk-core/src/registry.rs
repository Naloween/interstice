use interstice_abi::{ReducerSchema, SubscriptionSchema, TableSchema};
use std::sync::{Arc, Mutex};

#[repr(C)]
pub struct ReducerRegistration {
    pub reducer: unsafe extern "C" fn(i32, i32) -> i64,
    pub schema: fn() -> ReducerSchema,
}

#[repr(C)]
pub struct TableRegistration {}

#[repr(C)]
pub struct SubscriptionRegistration {}

pub type ReducerSchemaFn = fn() -> ReducerSchema;

lazy_static::lazy_static! {
    pub static ref REDUCER_REGISTRY: Arc<Mutex<Vec<ReducerSchemaFn>>> = Arc::new(Mutex::new(Vec::new()));
}

/// Called by each `#[reducer]` macro to register its schema function
pub fn register_reducer(f: ReducerSchemaFn) {
    REDUCER_REGISTRY.lock().unwrap().push(f);
}

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
