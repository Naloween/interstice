use crate::{Node, error::IntersticeError};
use interstice_abi::{CallReducerRequest, CallReducerResponse, ModuleSelection};

impl Node {
    pub(crate) fn handle_call_reducer(
        &mut self,
        caller_module_name: &String,
        call_reducer_request: CallReducerRequest,
    ) -> Result<CallReducerResponse, IntersticeError> {
        let module_name = match &call_reducer_request.module_selection {
            ModuleSelection::Current => caller_module_name,
            ModuleSelection::Other(name) => name,
        };
        let (result, events) = self.invoke_reducer(
            module_name,
            &call_reducer_request.reducer_name,
            call_reducer_request.input,
        )?;

        let frame = self.call_stack.last_mut().unwrap();
        frame.emitted_events.extend(events);
        Ok(result)
    }
}
