use crate::{error::IntersticeError, runtime::Runtime};
use interstice_abi::{CallReducerResponse, host::CallReducerRequest};

impl Runtime {
    pub(crate) fn handle_call_reducer(
        &mut self,
        call_reducer_request: CallReducerRequest,
    ) -> Result<CallReducerResponse, IntersticeError> {
        let (result, events) = self.invoke_reducer(
            &call_reducer_request.target_module,
            &call_reducer_request.reducer,
            call_reducer_request.input,
        )?;

        let frame = self.call_stack.last_mut().unwrap();
        frame.emitted_events.extend(events);
        Ok(result)
    }
}
