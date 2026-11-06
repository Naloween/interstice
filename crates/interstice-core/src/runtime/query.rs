use crate::{
    error::IntersticeError,
    runtime::Runtime,
    runtime::reducer::{CallFrame, CallFrameKind, CALL_STACK},
};
use interstice_abi::{IntersticeValue, RawQueryContext as QueryContext};
use serde::Serialize;

impl Runtime {
    pub(crate) fn call_query(
        &self,
        module_name: &str,
        query_name: &str,
        args: impl Serialize,
        caller_node_id: crate::node::NodeId,
        caller_module_name: &str,
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

        let query_schema = module
            .schema
            .queries
            .iter()
            .find(|q| q.name == query_name)
            .ok_or_else(|| IntersticeError::ReducerNotFound {
                module: module_name.into(),
                reducer: query_name.into(),
            })?;
        self.security
            .check_call(&module.schema, query_name, true, caller_node_id, caller_module_name)?;
        let table_access = crate::runtime::reducer::ReducerTableAccess::from_query_schema(query_schema);

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
        // A nested query belongs to the caller's transaction; an outermost
        // query is a read-only transaction of its own (DESIGN §6.5).
        let tx = CALL_STACK
            .with(|s| s.borrow().first().map(|f| f.tx))
            .unwrap_or_else(|| self.begin_tx(module_name, query_name, CallFrameKind::Query));

        // Share the caller's scope: blobs this query returns stay alive for
        // it. An outermost query gets a fresh scope.
        let scope = crate::runtime::blob::current_scope()
            .unwrap_or_else(|| crate::runtime::blob::BlobScope::new(None));
        CALL_STACK.with(|s| {
            s.borrow_mut().push(CallFrame::new(
                module_name.into(),
                query_name.into(),
                module.clone(),
                CallFrameKind::Query,
                tx,
                table_access,
                scope,
            ));
        });

        let query_context =
            QueryContext::new(
                caller_node_id.to_string(),
                caller_module_name.to_string(),
                module.state_epoch(),
            );
        let mut result = module.call_query(query_name, (query_context, args));
        // With no caller (host or remote request) the scope dies with this
        // frame, so blobs in the result travel inline.
        let outermost = CALL_STACK.with(|s| s.borrow().len() == 1);
        if outermost {
            if let Ok(value) = &mut result {
                if let Err(err) = crate::runtime::blob::inline_blobs(value) {
                    result = Err(IntersticeError::Internal(err).into());
                }
            }
        }
        // Popping the outermost query frame drops its pins.
        let frame = CALL_STACK.with(|s| s.borrow_mut().pop().unwrap());
        drop(frame);
        result
    }
}
