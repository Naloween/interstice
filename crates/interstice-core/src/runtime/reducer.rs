use crate::error::IntersticeError;
use crate::runtime::Runtime;
use interstice_abi::PrimitiveValue;
use interstice_abi::types::ModuleId;

#[derive(Debug)]
pub struct ReducerFrame {
    pub module: String,
    pub reducer: String,
}

impl Runtime {
    pub fn invoke_reducer(
        &mut self,
        module_name: &str,
        reducer_name: &str,
        args: PrimitiveValue,
    ) -> Result<PrimitiveValue, IntersticeError> {
        // Lookup module
        let module = self
            .modules
            .get_mut(module_name)
            .ok_or_else(|| IntersticeError::ModuleNotFound(module_name.into()))?;

        // Check that reducer exist in schema
        module
            .schema()
            .reducers
            .iter()
            .find(|r| r.name == reducer_name)
            .ok_or_else(|| IntersticeError::ReducerNotFound {
                module: module_name.into(),
                reducer: reducer_name.into(),
            })?;

        // Detect cycles
        if self
            .call_stack
            .iter()
            .any(|f| f.module == module_name && f.reducer == reducer_name)
        {
            return Err(IntersticeError::ReducerCycle {
                module: module_name.into(),
                reducer: reducer_name.into(),
            });
        }

        // Push frame
        self.call_stack.push(ReducerFrame {
            module: module_name.into(),
            reducer: reducer_name.into(),
        });

        // Call WASM function
        let result = module.call_reducer(reducer_name, args)?;

        // Pop frame
        self.call_stack.pop();

        Ok(result)
    }
}
