use crate::{
    error::IntersticeError,
    runtime::transaction::Transaction,
    runtime::{Runtime, table::TableAutoIncSnapshot},
};
use interstice_abi::{IntersticeValue, ReducerContext};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::mpsc;

#[derive(Debug, Clone)]
pub struct ReducerJob {
    pub module_name: String,
    pub reducer_name: String,
    pub input: IntersticeValue,
    pub completion: Option<mpsc::Sender<()>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallFrameKind {
    Reducer,
    Query,
}

#[derive(Debug)]
pub struct CallFrame {
    pub module: String,
    pub kind: CallFrameKind,
    pub transactions: Vec<Transaction>,
    pub auto_inc_snapshots: HashMap<String, TableAutoIncSnapshot>,
}

impl CallFrame {
    pub fn new(module: String, kind: CallFrameKind) -> Self {
        Self {
            module,
            kind,
            transactions: Vec::new(),
            auto_inc_snapshots: HashMap::new(),
        }
    }
}

pub struct CompletionGuard(Option<mpsc::Sender<()>>);

impl CompletionGuard {
    pub fn new(sender: mpsc::Sender<()>) -> Self {
        Self(Some(sender))
    }
}

impl Drop for CompletionGuard {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

impl Runtime {
    pub(crate) async fn call_reducer(
        &self,
        module_name: &str,
        reducer_name: &str,
        args: impl Serialize,
    ) -> Result<(), IntersticeError> {
        // Lookup module
        let module = {
            let mut modules = self.modules.lock().unwrap();
            modules
                .get_mut(module_name)
                .ok_or_else(|| {
                    IntersticeError::ModuleNotFound(
                        module_name.into(),
                        format!(
                            "When trying to invoke reducer '{}' from '{}'",
                            reducer_name, module_name
                        ),
                    )
                })?
                .clone()
        };

        // Check that reducer exists in schema
        module
            .schema
            .reducers
            .iter()
            .find(|r| r.name == reducer_name)
            .ok_or_else(|| IntersticeError::ReducerNotFound {
                module: module_name.into(),
                reducer: reducer_name.into(),
            })?;

        // Detect cycles (no module already called before)
        if self
            .call_stack
            .lock()
            .unwrap()
            .iter()
            .any(|f| f.module == module_name)
        {
            return Err(IntersticeError::ReducerCycle {
                module: module_name.into(),
                reducer: reducer_name.into(),
            });
        }

        // Push frame
        self.call_stack
            .lock()
            .unwrap()
            .push(CallFrame::new(module_name.into(), CallFrameKind::Reducer));

        // Call WASM function
        let reducer_context = ReducerContext::new();
        module
            .call_reducer(reducer_name, (reducer_context, args))
            .await?;

        // Pop frame
        let reducer_frame = self.call_stack.lock().unwrap().pop().unwrap();

        // Apply transactions
        let mut emitted_events = Vec::new();
        for transaction in reducer_frame.transactions {
            emitted_events.append(&mut self.apply_transaction(transaction, true)?);
        }

        // Send events
        for ev in emitted_events {
            self.event_sender.send(ev).unwrap();
        }

        Ok(())
    }
}
