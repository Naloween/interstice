use crate::error::IntersticeError;
use crate::runtime::Runtime;
use interstice_abi::PrimitiveValue;

#[derive(Debug)]
pub struct ReducerFrame {
    pub module: String,
    pub reducer: String,
}

impl Runtime {
    pub fn call_reducer(
        &mut self,
        module_name: &str,
        reducer_name: &str,
        args: PrimitiveValue,
    ) -> Result<PrimitiveValue, IntersticeError> {
        // 1. Lookup module
        let module = self
            .modules
            .get_mut(module_name)
            .ok_or_else(|| IntersticeError::ModuleNotFound(module_name.into()))?;

        module
            .schema()
            .reducers
            .iter()
            .find(|r| r.name == reducer_name)
            .ok_or_else(|| IntersticeError::ReducerNotFound {
                module: module_name.into(),
                reducer: reducer_name.into(),
            })?;

        // 2. Detect cycles
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

        // 3. Push frame
        self.call_stack.push(ReducerFrame {
            module: module_name.into(),
            reducer: reducer_name.into(),
        });

        // 4. Call WASM function
        let result = module.instance_mut().call_reducer(reducer_name, args);

        // 5. Pop frame
        self.call_stack.pop();

        result
    }
}
