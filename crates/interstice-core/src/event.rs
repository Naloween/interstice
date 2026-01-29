use crate::{Node, error::IntersticeError, subscription::SubscriptionTarget};
use interstice_abi::{IntersticeValue, Row, SubscriptionEventSchema};

#[derive(Debug, Clone)]
pub enum SubscriptionEventInstance {
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
    Init {
        module_name: String,
    },
}

impl SubscriptionEventInstance {
    pub fn has_schema(&self, event_schema: &SubscriptionEventSchema) -> bool {
        match &self {
            SubscriptionEventInstance::TableInsertEvent {
                module_name,
                table_name,
                ..
            } => {
                if let SubscriptionEventSchema::Insert {
                    module_name: module_name_schema,
                    table_name: table_name_schema,
                } = event_schema
                {
                    return module_name == module_name_schema && table_name == table_name_schema;
                } else {
                    return false;
                }
            }
            SubscriptionEventInstance::TableUpdateEvent {
                module_name,
                table_name,
                ..
            } => {
                if let SubscriptionEventSchema::Update {
                    module_name: module_name_schema,
                    table_name: table_name_schema,
                } = event_schema
                {
                    return module_name == module_name_schema && table_name == table_name_schema;
                } else {
                    return false;
                }
            }
            SubscriptionEventInstance::TableDeleteEvent {
                module_name,
                table_name,
                ..
            } => {
                if let SubscriptionEventSchema::Delete {
                    module_name: module_name_schema,
                    table_name: table_name_schema,
                } = event_schema
                {
                    return module_name == module_name_schema && table_name == table_name_schema;
                } else {
                    return false;
                }
            }
            SubscriptionEventInstance::Init { .. } => {
                if let SubscriptionEventSchema::Init = event_schema {
                    return true;
                } else {
                    return false;
                }
            }
        }
    }
}

impl Node {
    pub(crate) fn process_event_queue(&mut self) -> Result<(), IntersticeError> {
        while let Some(event) = self.event_queue.pop_front() {
            let triggered = self.find_subscriptions(&event)?;

            for sub in triggered {
                let ((), new_events) = self.invoke_subscription(sub, event.clone())?;
                self.event_queue.extend(new_events);
            }
        }

        Ok(())
    }

    fn find_subscriptions(
        &self,
        event: &SubscriptionEventInstance,
    ) -> Result<Vec<SubscriptionTarget>, IntersticeError> {
        let mut out = Vec::new();

        if let SubscriptionEventInstance::Init { module_name } = event {
            let module = self.modules.get(module_name).unwrap();
            for sub in &module.schema.subscriptions {
                if event.has_schema(&sub.event) {
                    out.push(SubscriptionTarget {
                        module: module.schema.name.clone(),
                        reducer: sub.reducer_name.clone(),
                    });
                }
            }
        } else {
            for module in self.modules.values() {
                for sub in &module.schema.subscriptions {
                    if event.has_schema(&sub.event) {
                        out.push(SubscriptionTarget {
                            module: module.schema.name.clone(),
                            reducer: sub.reducer_name.clone(),
                        });
                    }
                }
            }
        }

        Ok(out)
    }

    fn invoke_subscription(
        &mut self,
        target: SubscriptionTarget,
        event: SubscriptionEventInstance,
    ) -> Result<((), Vec<SubscriptionEventInstance>), IntersticeError> {
        let args = match event {
            SubscriptionEventInstance::TableInsertEvent {
                module_name: _,
                table_name: _,
                inserted_row,
            } => IntersticeValue::Vec(vec![inserted_row.into()]),
            SubscriptionEventInstance::TableUpdateEvent {
                module_name: _,
                table_name: _,
                old_row,
                new_row,
            } => IntersticeValue::Vec(vec![old_row.into(), new_row.into()]),
            SubscriptionEventInstance::TableDeleteEvent {
                module_name: _,
                table_name: _,
                deleted_row,
            } => IntersticeValue::Vec(vec![deleted_row.into()]),
            SubscriptionEventInstance::Init { .. } => IntersticeValue::Vec(vec![]),
        };

        let (_ret, events) = self.invoke_reducer(&target.module, &target.reducer, args)?;
        Ok(((), events))
    }
}
