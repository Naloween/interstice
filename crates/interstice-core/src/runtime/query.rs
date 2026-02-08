use crate::{
    IntersticeError,
    runtime::{
        Runtime,
        reducer::{CallFrame, CallFrameKind},
    },
};
use interstice_abi::{IntersticeValue, QueryContext};
use serde::Serialize;

impl Runtime {
    pub(crate) fn call_query(
        &self,
        module_name: &str,
        query_name: &str,
        args: impl Serialize,
    ) -> Result<IntersticeValue, IntersticeError> {
        // Lookup module
        let mut modules = self.modules.lock().unwrap();
        let module = modules
            .get_mut(module_name)
            .ok_or_else(|| {
                IntersticeError::ModuleNotFound(
                    module_name.into(),
                    format!(
                        "When trying to invoke query '{}' from '{}'",
                        query_name, module_name
                    ),
                )
            })?
            .clone();
        drop(modules);

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
                reducer: query_name.into(),
            });
        }

        // Push frame
        self.call_stack
            .lock()
            .unwrap()
            .push(CallFrame::new(module_name.into(), CallFrameKind::Query));

        // Call WASM function
        let query_context = QueryContext::new();
        let result = module.call_query(query_name, (query_context, args))?;

        // Pop frame
        let _ = self.call_stack.lock().unwrap().pop().unwrap();

        Ok(result)
    }
}
