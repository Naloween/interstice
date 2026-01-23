mod module;
mod reducer;
mod table;

use crate::{
    error::IntersticeError,
    runtime::{module::Module, reducer::ReducerFrame, table::TableEventInstance},
    wasm::{StoreState, linker::define_host_calls},
};
use interstice_abi::IntersticeValue;
use std::collections::VecDeque;
use std::{collections::HashMap, sync::Arc};
use wasmtime::{Engine, Linker};

pub struct Runtime {
    pub(crate) modules: HashMap<String, Module>,
    pub(crate) call_stack: Vec<ReducerFrame>,
    engine: Arc<Engine>,
    linker: Linker<StoreState>,
}

impl Runtime {
    pub fn new() -> Self {
        let engine = Arc::new(Engine::default());
        let mut linker = Linker::new(&engine);
        define_host_calls(&mut linker).expect("Couldn't add host calls to the linker");
        Self {
            modules: HashMap::new(),
            call_stack: Vec::new(),
            engine,
            linker,
        }
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
}

struct SubscriptionTarget {
    module: String,
    reducer: String,
}
