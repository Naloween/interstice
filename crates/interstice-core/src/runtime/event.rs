use crate::{
    error::IntersticeError,
    network::protocol::{NetworkPacket, TableEventInstance},
    node::NodeId,
    runtime::{Runtime, authority::AuthorityEntry},
};
use interstice_abi::{
    Authority, FileEvent, InputEvent, IntersticeValue, ModuleEvent, Row, SubscriptionEventSchema,
};

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
    Load {
        module_name: String,
    },
    Input(InputEvent),
    AudioOutput,
    AudioInput {
        stream_id: u64,
        data: Vec<Vec<f32>>,
    },
    File(FileEvent),
    Module(ModuleEvent),
    RequestAppInitialization,
    AppInitialized,
    RequestSubscription {
        requesting_node_id: NodeId,
        event: SubscriptionEventSchema,
    },
    RemoteReducerCall {
        requesting_node_id: NodeId,
        module_name: String,
        reducer_name: String,
        input: IntersticeValue,
    },
    RemoteQueryCall {
        requesting_node_id: NodeId,
        request_id: String,
        module_name: String,
        query_name: String,
        input: IntersticeValue,
    },
    RemoteQueryResponse {
        request_id: String,
        result: IntersticeValue,
    },
    PublishModule {
        wasm_binary: Vec<u8>,
        source_node_id: NodeId,
    },
    RemoveModule {
        module_name: String,
        source_node_id: NodeId,
    },
    SchemaRequest {
        requesting_node_id: NodeId,
        request_id: String,
        node_name: String,
    },
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
                    ..
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
                    ..
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
                    ..
                } = event_schema
                {
                    return module_name == module_name_schema && table_name == table_name_schema;
                } else {
                    return false;
                }
            }
            EventInstance::Init { .. } => {
                if let SubscriptionEventSchema::Init = event_schema {
                    return true;
                } else {
                    return false;
                }
            }
            EventInstance::Load { .. } => {
                if let SubscriptionEventSchema::Load = event_schema {
                    return true;
                } else {
                    return false;
                }
            }
            EventInstance::Input(_input_event) => {
                if let SubscriptionEventSchema::Input = event_schema {
                    return true;
                } else {
                    return false;
                }
            }
            EventInstance::AudioOutput => {
                if let SubscriptionEventSchema::AudioOutput = event_schema {
                    return true;
                } else {
                    return false;
                }
            }
            EventInstance::AudioInput { .. } => {
                if let SubscriptionEventSchema::AudioInput = event_schema {
                    return true;
                } else {
                    return false;
                }
            }
            EventInstance::File(file_event) => {
                if let SubscriptionEventSchema::File { path, recursive } = event_schema {
                    let event_path = std::path::Path::new(match file_event {
                        FileEvent::Created { path }
                        | FileEvent::Modified { path }
                        | FileEvent::Deleted { path } => path,
                        FileEvent::Renamed { to, .. } => to,
                    });
                    let schema_path = std::path::Path::new(path);

                    if *recursive {
                        return event_path.starts_with(schema_path);
                    }
                    return event_path == schema_path;
                } else {
                    return false;
                }
            }
            EventInstance::Module(module_event) => match (module_event, event_schema) {
                (ModuleEvent::PublishRequest { .. }, SubscriptionEventSchema::ModulePublish) => {
                    true
                }
                (ModuleEvent::RemoveRequest { .. }, SubscriptionEventSchema::ModuleRemove) => true,
                _ => false,
            },
            _ => false,
        }
    }
}

#[derive(Debug)]
pub enum SubscriptionTarget {
    Local { module: String, reducer: String },
    Remote(NodeId),
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
                    out.push(SubscriptionTarget::Local {
                        module: module.schema.name.clone(),
                        reducer: sub.reducer_name.clone(),
                    });
                }
            }
        } else if let EventInstance::Load { module_name } = event {
            let module = self.modules.lock().unwrap();
            let module = module.get(module_name).unwrap();
            for sub in &module.schema.subscriptions {
                if event.has_schema(&sub.event) {
                    out.push(SubscriptionTarget::Local {
                        module: module.schema.name.clone(),
                        reducer: sub.reducer_name.clone(),
                    });
                }
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
                out.push(SubscriptionTarget::Local {
                    module: module_name,
                    reducer: on_input_reducer_name,
                });
            }
        } else if let EventInstance::AudioOutput = event {
            if let Some(AuthorityEntry {
                module_name,
                on_event_reducer_name: Some(on_audio_reducer_name),
            }) = self
                .authority_modules
                .lock()
                .unwrap()
                .get(&Authority::Audio)
                .cloned()
            {
                out.push(SubscriptionTarget::Local {
                    module: module_name,
                    reducer: on_audio_reducer_name,
                });
            }
        } else if let EventInstance::AudioInput { .. } = event {
            if let Some(AuthorityEntry {
                module_name,
                on_event_reducer_name: Some(on_audio_reducer_name),
            }) = self
                .authority_modules
                .lock()
                .unwrap()
                .get(&Authority::Audio)
                .cloned()
            {
                out.push(SubscriptionTarget::Local {
                    module: module_name,
                    reducer: on_audio_reducer_name,
                });
            }
        } else if let EventInstance::File(_) = event {
            for module in self.modules.lock().unwrap().values() {
                for sub in &module.schema.subscriptions {
                    if event.has_schema(&sub.event) {
                        out.push(SubscriptionTarget::Local {
                            module: module.schema.name.clone(),
                            reducer: sub.reducer_name.clone(),
                        });
                    }
                }
            }
        } else if let EventInstance::Module(_) = event {
            for module in self.modules.lock().unwrap().values() {
                for sub in &module.schema.subscriptions {
                    if event.has_schema(&sub.event) {
                        out.push(SubscriptionTarget::Local {
                            module: module.schema.name.clone(),
                            reducer: sub.reducer_name.clone(),
                        });
                    }
                }
            }
        } else {
            for module in self.modules.lock().unwrap().values() {
                for sub in &module.schema.subscriptions {
                    if event.has_schema(&sub.event) {
                        out.push(SubscriptionTarget::Local {
                            module: module.schema.name.clone(),
                            reducer: sub.reducer_name.clone(),
                        });
                    }
                }
            }

            for (node_id, subscriptions) in self.node_subscriptions.lock().unwrap().iter() {
                for sub in subscriptions {
                    if event.has_schema(&sub) {
                        out.push(SubscriptionTarget::Remote(*node_id));
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
        match target {
            SubscriptionTarget::Local { module, reducer } => {
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
                    EventInstance::Input(input_event) => {
                        IntersticeValue::Vec(vec![input_event.into()])
                    }
                    EventInstance::AudioInput { stream_id, data } => {
                        IntersticeValue::Vec(vec![stream_id.into(), data.into()])
                    }
                    EventInstance::File(file_event) => {
                        IntersticeValue::Vec(vec![file_event.into()])
                    }
                    EventInstance::Module(module_event) => {
                        IntersticeValue::Vec(vec![module_event.into()])
                    }
                    _ => IntersticeValue::Vec(vec![]),
                };
                let _ = self.reducer_sender.send(crate::runtime::ReducerJob {
                    module_name: module,
                    reducer_name: reducer,
                    input: args,
                    caller_node_id: self.network_handle.node_id,
                    completion: None,
                });
            }
            SubscriptionTarget::Remote(uuid) => {
                let packet = match event {
                    EventInstance::TableInsertEvent {
                        module_name,
                        table_name,
                        inserted_row,
                    } => NetworkPacket::TableEvent(TableEventInstance::TableInsertEvent {
                        module_name,
                        table_name,
                        inserted_row,
                    }),
                    EventInstance::TableUpdateEvent {
                        module_name,
                        table_name,
                        old_row,
                        new_row,
                    } => NetworkPacket::TableEvent(TableEventInstance::TableUpdateEvent {
                        module_name,
                        table_name,
                        old_row,
                        new_row,
                    }),
                    EventInstance::TableDeleteEvent {
                        module_name,
                        table_name,
                        deleted_row,
                    } => NetworkPacket::TableEvent(TableEventInstance::TableDeleteEvent {
                        module_name,
                        table_name,
                        deleted_row,
                    }),
                    EventInstance::File(_) | EventInstance::Input(_) | EventInstance::Module(_) => {
                        return Ok(());
                    }
                    event => {
                        return Err(IntersticeError::Internal(format!(
                            "Tried to send an event {:?} to remote node {}, which is not supported",
                            event, uuid
                        )));
                    }
                };
                self.network_handle.send_packet(uuid, packet);
            }
        }

        Ok(())
    }
}
