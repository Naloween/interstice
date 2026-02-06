use crate::{
    error::IntersticeError,
    network::protocol::RequestSubscription,
    runtime::{Runtime, authority::AuthorityEntry},
};
use interstice_abi::{Authority, InputEvent, IntersticeValue, Row, SubscriptionEventSchema};

#[derive(Debug, Clone)]
pub enum EventInstance {
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
    Render,
    Input(InputEvent),
    AppInitialized,
    RequestSubscription(RequestSubscription),
}

impl EventInstance {
    pub fn has_schema(&self, event_schema: &SubscriptionEventSchema) -> bool {
        match &self {
            EventInstance::TableInsertEvent {
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
            EventInstance::TableUpdateEvent {
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
            EventInstance::TableDeleteEvent {
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
            EventInstance::Init { module_name } => {
                if let SubscriptionEventSchema::Init = event_schema {
                    return true;
                } else {
                    return false;
                }
            }
            EventInstance::Render => {
                if let SubscriptionEventSchema::Render = event_schema {
                    return true;
                } else {
                    return false;
                }
            }
            EventInstance::Input(input_event) => {
                if let SubscriptionEventSchema::Input = event_schema {
                    return true;
                } else {
                    return false;
                }
            }
            _ => false,
        }
    }
}

#[derive(Debug)]
pub struct SubscriptionTarget {
    pub module: String,
    pub reducer: String,
}

impl Runtime {
    pub(crate) fn find_subscriptions(
        &self,
        event: &EventInstance,
    ) -> Result<Vec<SubscriptionTarget>, IntersticeError> {
        let mut out = Vec::new();

        if let EventInstance::Init { module_name } = event {
            let module = self.modules.lock().unwrap();
            let module = module.get(module_name).unwrap();
            for sub in &module.schema.subscriptions {
                if event.has_schema(&sub.event) {
                    out.push(SubscriptionTarget {
                        module: module.schema.name.clone(),
                        reducer: sub.reducer_name.clone(),
                    });
                }
            }
        } else if let EventInstance::Render = event {
            if let Some(AuthorityEntry {
                module_name: gpu_module_name,
                on_event_reducer_name: Some(render_reducer_name),
            }) = self
                .authority_modules
                .lock()
                .unwrap()
                .get(&Authority::Gpu)
                .cloned()
            {
                out.push(SubscriptionTarget {
                    module: gpu_module_name,
                    reducer: render_reducer_name,
                });
            }
        } else if let EventInstance::Input(_) = event {
            if let Some(AuthorityEntry {
                module_name,
                on_event_reducer_name: Some(on_input_reducer_name),
            }) = self
                .authority_modules
                .lock()
                .unwrap()
                .get(&Authority::Input)
                .cloned()
            {
                out.push(SubscriptionTarget {
                    module: module_name,
                    reducer: on_input_reducer_name,
                });
            }
        } else {
            for module in self.modules.lock().unwrap().values() {
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

    pub(crate) fn invoke_subscription(
        &self,
        target: SubscriptionTarget,
        event: EventInstance,
    ) -> Result<(), IntersticeError> {
        let args = match event {
            EventInstance::TableInsertEvent {
                module_name: _,
                table_name: _,
                inserted_row,
            } => IntersticeValue::Vec(vec![inserted_row.into()]),
            EventInstance::TableUpdateEvent {
                module_name: _,
                table_name: _,
                old_row,
                new_row,
            } => IntersticeValue::Vec(vec![old_row.into(), new_row.into()]),
            EventInstance::TableDeleteEvent {
                module_name: _,
                table_name: _,
                deleted_row,
            } => IntersticeValue::Vec(vec![deleted_row.into()]),
            EventInstance::Input(input_event) => IntersticeValue::Vec(vec![input_event.into()]),
            _ => IntersticeValue::Vec(vec![]),
        };
        let _ret = self.call_reducer(&target.module, &target.reducer, args)?;
        Ok(())
    }
}
