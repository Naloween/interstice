use std::collections::VecDeque;

use interstice_abi::{IntersticeValue, Row, TableEvent};

use crate::{Runtime, error::IntersticeError, runtime::SubscriptionTarget};

#[derive(Debug, Clone)]
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

impl TableEventInstance {
    pub fn get_event(&self) -> TableEvent {
        match self {
            TableEventInstance::TableInsertEvent {
                module_name: _,
                table_name: _,
                inserted_row: _,
            } => TableEvent::Insert,
            TableEventInstance::TableUpdateEvent {
                module_name: _,
                table_name: _,
                old_row: _,
                new_row: _,
            } => TableEvent::Update,
            TableEventInstance::TableDeleteEvent {
                module_name: _,
                table_name: _,
                deleted_row: _,
            } => TableEvent::Delete,
        }
    }

    pub fn get_module_name(&self) -> &String {
        match self {
            TableEventInstance::TableInsertEvent {
                module_name,
                table_name: _,
                inserted_row: _,
            } => module_name,
            TableEventInstance::TableUpdateEvent {
                module_name,
                table_name: _,
                old_row: _,
                new_row: _,
            } => module_name,
            TableEventInstance::TableDeleteEvent {
                module_name,
                table_name: _,
                deleted_row: _,
            } => module_name,
        }
    }

    pub fn get_table_name(&self) -> &String {
        match self {
            TableEventInstance::TableInsertEvent {
                module_name: _,
                table_name,
                inserted_row: _,
            } => table_name,
            TableEventInstance::TableUpdateEvent {
                module_name: _,
                table_name,
                old_row: _,
                new_row: _,
            } => table_name,
            TableEventInstance::TableDeleteEvent {
                module_name: _,
                table_name,
                deleted_row: _,
            } => table_name,
        }
    }
}

impl Runtime {
    pub(crate) fn process_event_queue(
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
