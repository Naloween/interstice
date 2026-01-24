mod module;
mod reducer;
mod table;

use crate::{
    error::IntersticeError,
    persistence::{PersistenceConfig, Transaction, TransactionLog, TransactionType},
    runtime::{module::Module, reducer::ReducerFrame, table::TableEventInstance},
    wasm::{StoreState, linker::define_host_calls},
};
use interstice_abi::IntersticeValue;
use std::collections::VecDeque;
use std::{collections::HashMap, sync::Arc};
use wasmtime::{Engine, Linker};

/// Main Interstice runtime that executes modules and manages state.
///
/// The runtime can optionally log all table mutations for durability and replay.
/// Mutations are logged atomically before being acknowledged to modules.
pub struct Runtime {
    pub(crate) modules: HashMap<String, Module>,
    pub(crate) call_stack: Vec<ReducerFrame>,
    engine: Arc<Engine>,
    linker: Linker<StoreState>,
    /// Optional transaction log for durable persistence
    transaction_log: Option<TransactionLog>,
    /// Logical clock for transaction timestamps
    tx_clock: u64,
}

impl Runtime {
    /// Create a runtime with in-memory state only (no persistence)
    pub fn new() -> Self {
        let engine = Arc::new(Engine::default());
        let mut linker = Linker::new(&engine);
        define_host_calls(&mut linker).expect("Couldn't add host calls to the linker");
        Self {
            modules: HashMap::new(),
            call_stack: Vec::new(),
            engine,
            linker,
            transaction_log: None,
            tx_clock: 0,
        }
    }

    /// Create a runtime with optional transaction logging
    pub fn with_persistence(config: PersistenceConfig) -> std::io::Result<Self> {
        let engine = Arc::new(Engine::default());
        let mut linker = Linker::new(&engine);
        define_host_calls(&mut linker).expect("Couldn't add host calls to the linker");

        let transaction_log = if config.enabled {
            // Create log directory if it doesn't exist
            std::fs::create_dir_all(&config.log_dir)?;
            Some(TransactionLog::new(config.log_file_path())?)
        } else {
            None
        };

        Ok(Self {
            modules: HashMap::new(),
            call_stack: Vec::new(),
            engine,
            linker,
            transaction_log,
            tx_clock: 0,
        })
    }

    pub fn run(
        &mut self,
        module: &str,
        reducer: &str,
        args: IntersticeValue,
    ) -> Result<IntersticeValue, IntersticeError> {
        let mut event_queue = VecDeque::<TableEventInstance>::new();

        // 1. Call root reducer
        let (result, events) = self.invoke_reducer(module, reducer, args)?;
        event_queue.extend(events);

        // 2. Process subscriptions
        self.process_event_queue(&mut event_queue)?;

        // 3. Return root result
        Ok(result)
    }

    fn process_event_queue(
        &mut self,
        event_queue: &mut VecDeque<TableEventInstance>,
    ) -> Result<(), IntersticeError> {
        while let Some(event) = event_queue.pop_front() {
            let triggered = self.find_subscriptions(&event)?;

            for sub in triggered {
                let ((), new_events) = self.invoke_subscription(sub, event.clone())?;
                event_queue.extend(new_events);
            }
        }

        Ok(())
    }

    fn find_subscriptions(
        &self,
        event: &TableEventInstance,
    ) -> Result<Vec<SubscriptionTarget>, IntersticeError> {
        let mut out = Vec::new();

        for module in self.modules.values() {
            for sub in &module.schema.subscriptions {
                if sub.event == event.get_event()
                    && &sub.table_name == event.get_table_name()
                    && &sub.module_name == event.get_module_name()
                {
                    out.push(SubscriptionTarget {
                        module: module.schema.name.clone(),
                        reducer: sub.reducer_name.clone(),
                    });
                }
            }
        }

        Ok(out)
    }

    fn invoke_subscription(
        &mut self,
        target: SubscriptionTarget,
        event: TableEventInstance,
    ) -> Result<((), Vec<TableEventInstance>), IntersticeError> {
        let args = match event {
            TableEventInstance::TableInsertEvent {
                module_name: _,
                table_name: _,
                inserted_row,
            } => IntersticeValue::Vec(vec![inserted_row.into()]),
            TableEventInstance::TableUpdateEvent {
                module_name: _,
                table_name: _,
                old_row,
                new_row,
            } => IntersticeValue::Vec(vec![old_row.into(), new_row.into()]),
            TableEventInstance::TableDeleteEvent {
                module_name: _,
                table_name: _,
                deleted_row,
            } => IntersticeValue::Vec(vec![deleted_row.into()]),
        };

        let (_ret, events) = self.invoke_reducer(&target.module, &target.reducer, args)?;
        Ok(((), events))
    }

    /// Log a table mutation to the transaction log (if enabled).
    /// Increments the logical clock for ordering.
    pub(crate) fn log_mutation(
        &mut self,
        module_name: String,
        table_name: String,
        tx_type: TransactionType,
        row: interstice_abi::Row,
        old_row: Option<interstice_abi::Row>,
    ) -> Result<(), IntersticeError> {
        if let Some(ref mut log) = self.transaction_log {
            let tx = Transaction {
                transaction_type: tx_type,
                module_name,
                table_name,
                row,
                old_row,
                timestamp: self.tx_clock,
            };
            log.append(&tx).map_err(|_| IntersticeError::Internal("Failed to write transaction log"))?;
            self.tx_clock += 1;
        }
        Ok(())
    }
}

struct SubscriptionTarget {
    module: String,
    reducer: String,
}
