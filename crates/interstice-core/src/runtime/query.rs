use crate::{
    error::IntersticeError,
    runtime::Runtime,
    runtime::reducer::{CallFrame, CallFrameKind, CALL_STACK},
};
use interstice_abi::{IntersticeValue, QueryContext};
use serde::Serialize;

impl Runtime {
    pub(crate) fn call_query(
        &self,
        module_name: &str,
        query_name: &str,
        args: impl Serialize,
        caller_node_id: crate::node::NodeId,
    ) -> Result<IntersticeValue, IntersticeError> {
        let module = {
            let modules = self.modules.lock();
            modules
                .get(module_name)
                .ok_or_else(|| {
                    IntersticeError::ModuleNotFound(
                        module_name.into(),
                        format!(
                            "When trying to invoke query '{}' from '{}'",
                            query_name, module_name
                        ),
                    )
                })?
                .clone()
        };

        // Check that query exists in schema
        module
            .schema
            .queries
            .iter()
            .find(|q| q.name == query_name)
            .ok_or_else(|| IntersticeError::ReducerNotFound {
                module: module_name.into(),
                reducer: query_name.into(),
            })?;

        // Detect re-entrant query cycles — check the current thread's stack only.
        let cycle = CALL_STACK.with(|s| {
            s.borrow()
                .iter()
                .any(|f| f.module == module_name && f.kind == CallFrameKind::Query)
        });
        if cycle {
            return Err(IntersticeError::ReducerCycle {
                module: module_name.into(),
                reducer: query_name.into(),
            });
        }

        // Push frame onto current thread's stack (no lock needed — TLS).
        let call_sequence = self
            .call_sequence
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let rng_seed = crate::runtime::deterministic_random::seed_from_call(
            &caller_node_id,
            module_name,
            query_name,
            CallFrameKind::Query,
            call_sequence,
        );

        CALL_STACK.with(|s| {
            s.borrow_mut().push(CallFrame::new(
                module_name.into(),
                query_name.into(),
                module.clone(),
                CallFrameKind::Query,
                rng_seed,
                crate::runtime::reducer::ReducerTableAccess::default(),
            ));
        });

        let query_context = QueryContext::new(caller_node_id.to_string());
        let result = module.call_query(query_name, (query_context, args));
        CALL_STACK.with(|s| { s.borrow_mut().pop(); });
        result
    }
}
