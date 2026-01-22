use crate::{error::IntersticeError, runtime::Runtime};
use interstice_abi::{CallReducerResponse, host::CallReducerRequest};

impl Runtime {
    pub fn handle_call_reducer(
        &mut self,
        call_reducer_request: CallReducerRequest,
    ) -> Result<CallReducerResponse, IntersticeError> {
        return self.invoke_reducer(
            &call_reducer_request.target_module,
            &call_reducer_request.reducer,
            call_reducer_request.input,
        );
    }
}
