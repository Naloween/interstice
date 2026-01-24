mod event;
mod module;
mod reducer;
mod table;
pub mod transaction;

use crate::{
    error::IntersticeError,
    persistence::TransactionLog,
    runtime::{event::TableEventInstance, module::Module, reducer::ReducerFrame},
    wasm::{StoreState, linker::define_host_calls},
};
use interstice_abi::IntersticeValue;
use std::{collections::HashMap, sync::Arc};
use std::{collections::VecDeque, path::Path};
use wasmtime::{Engine, Linker};

pub struct Runtime {
    pub(crate) modules: HashMap<String, Module>,
    pub(crate) call_stack: Vec<ReducerFrame>,
    engine: Arc<Engine>,
    linker: Linker<StoreState>,
    transaction_logs: TransactionLog,
}

impl Runtime {
    pub fn new(transaction_log_path: &Path) -> Result<Self, IntersticeError> {
        let engine = Arc::new(Engine::default());
        let mut linker = Linker::new(&engine);
        define_host_calls(&mut linker).map_err(|err| {
            IntersticeError::Internal(format!("Couldn't add host calls to the linker: {}", err))
        })?;
        Ok(Self {
            modules: HashMap::new(),
            call_stack: Vec::new(),
            engine,
            linker,
            transaction_logs: TransactionLog::new(transaction_log_path)?,
        })
    }

    pub fn clear_logs(&mut self) -> Result<(), IntersticeError> {
        self.transaction_logs.delete_all_logs()?;
        Ok(())
    }

    pub fn run(
        &mut self,
        module: &str,
        reducer: &str,
        args: IntersticeValue,
    ) -> Result<IntersticeValue, IntersticeError> {
        // Replay previous logged transactions
        self.replay()?;

        let mut event_queue = VecDeque::<TableEventInstance>::new();

        // 1. Call root reducer
        let (result, events) = self.invoke_reducer(module, reducer, args)?;
        event_queue.extend(events);

        // 2. Process subscriptions
        self.process_event_queue(&mut event_queue)?;

        // 3. Return root result
        Ok(result)
    }

    fn replay(&mut self) -> Result<(), IntersticeError> {
        let transactions = self.transaction_logs.read_all()?;
        println!("Replaying transactions: {:?}", transactions);

        for transaction in transactions {
            let _events = self.apply_transaction(transaction)?;
        }

        Ok(())
    }
}

struct SubscriptionTarget {
    module: String,
    reducer: String,
}
