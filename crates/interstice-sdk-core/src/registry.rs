use interstice_abi::{
    ReducerSchema, SubscriptionSchema, TableSchema, interstice_type_def::IntersticeTypeDef,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use crate::host_calls::log;

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
pub type IntersticeTypeDefFn = fn() -> IntersticeTypeDef;

lazy_static::lazy_static! {
    pub static ref TABLE_REGISTRY: Arc<Mutex<Vec<TableSchemaFn>>> = Arc::new(Mutex::new(Vec::new()));
    pub static ref REDUCER_REGISTRY: Arc<Mutex<Vec<ReducerSchemaFn>>> = Arc::new(Mutex::new(Vec::new()));
    pub static ref SUBSCRIPTION_REGISTRY: Arc<Mutex<Vec<SubscriptionSchemaFn>>> = Arc::new(Mutex::new(Vec::new()));
    pub static ref INTERSTICE_TYPE_DEFINITION_REGISTRY: Arc<Mutex<Vec<IntersticeTypeDefFn>>> = Arc::new(Mutex::new(Vec::new()));
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

/// Called by each `#[derive(IntersticeType)]` macro to register its TypeDef function
pub fn register_type_def(s: IntersticeTypeDefFn) {
    INTERSTICE_TYPE_DEFINITION_REGISTRY.lock().unwrap().push(s);
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

pub fn collect_type_definitions() -> HashMap<String, IntersticeTypeDef> {
    let mut result = HashMap::new();
    for interstice_type_def_fn in INTERSTICE_TYPE_DEFINITION_REGISTRY.lock().unwrap().iter() {
        let interstice_type_def = interstice_type_def_fn();
        result.insert(interstice_type_def.get_name().clone(), interstice_type_def);
    }
    return result;
}
