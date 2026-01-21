use interstice_abi::{codec::pack_ptr_len, encode, host::CallReducerRequest};
use wasmtime::{Caller, Memory};

use crate::{runtime::Runtime, wasm::StoreState};

impl Runtime {
    pub fn handle_call_reducer(
        &mut self,
        memory: &Memory,
        caller: &mut Caller<'_, StoreState>,
        call_reducer_request: CallReducerRequest,
    ) -> i64 {
        let result = match self.invoke_reducer(
            &call_reducer_request.target_module,
            &call_reducer_request.reducer,
            call_reducer_request.input,
        ) {
            Ok(v) => v,
            Err(_) => return 0,
        };

        let encoded = encode(&result).unwrap();

        let (ptr, len) = self.allocate_return(memory, caller, &encoded);
        pack_ptr_len(ptr, len)
    }

    fn allocate_return(
        &self,
        memory: &Memory,
        mut caller: &mut Caller<'_, StoreState>,
        bytes: &[u8],
    ) -> (i32, i32) {
        let alloc = caller
            .get_export("alloc")
            .unwrap()
            .into_func()
            .unwrap()
            .typed::<i32, i32>(&caller)
            .unwrap();

        let ptr = alloc.call(&mut caller, bytes.len() as i32).unwrap();

        memory.write(&mut caller, ptr as usize, bytes).unwrap();

        (ptr, bytes.len() as i32)
    }
}
