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

pub type TableSchemaFn = fn() -> TableSchema;
pub type ReducerSchemaFn = fn() -> ReducerSchema;
pub type SubscriptionSchemaFn = fn() -> SubscriptionSchema;

lazy_static::lazy_static! {
    pub static ref TABLE_REGISTRY: Arc<Mutex<Vec<TableSchemaFn>>> = Arc::new(Mutex::new(Vec::new()));
    pub static ref REDUCER_REGISTRY: Arc<Mutex<Vec<ReducerSchemaFn>>> = Arc::new(Mutex::new(Vec::new()));
    pub static ref SUBSCRIPTION_REGISTRY: Arc<Mutex<Vec<SubscriptionSchemaFn>>> = Arc::new(Mutex::new(Vec::new()));
}

/// Called by each `#[table]` macro to register its schema function
pub fn register_table(f: TableSchemaFn) {
    TABLE_REGISTRY.lock().unwrap().push(f);
}

/// Called by each `#[reducer]` macro to register its schema function
pub fn register_reducer(f: ReducerSchemaFn) {
    REDUCER_REGISTRY.lock().unwrap().push(f);
}

/// Called by each `#[reducer]` macro to register its potential subscription schema function
pub fn register_subscription(s: SubscriptionSchemaFn) {
    SUBSCRIPTION_REGISTRY.lock().unwrap().push(s);
}

pub fn collect_tables() -> Vec<TableSchema> {
    TABLE_REGISTRY.lock().unwrap().iter().map(|f| f()).collect()
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
    SUBSCRIPTION_REGISTRY
        .lock()
        .unwrap()
        .iter()
        .map(|f| f())
        .collect()
}
