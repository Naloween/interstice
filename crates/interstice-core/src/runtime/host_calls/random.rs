use crate::runtime::Runtime;
use crate::runtime::deterministic_random::next_u64;
use interstice_abi::{DeterministicRandomRequest, DeterministicRandomResponse};

impl Runtime {
    pub(crate) fn handle_deterministic_random(
        &self,
        _request: DeterministicRandomRequest,
    ) -> DeterministicRandomResponse {
        let mut call_stack = self.call_stack.lock().unwrap();
        let frame = match call_stack.last_mut() {
            Some(frame) => frame,
            None => {
                return DeterministicRandomResponse::Err(
                    "Deterministic random call outside of an active frame".into(),
                );
            }
        };

        let value = next_u64(&mut frame.rng_state);
        DeterministicRandomResponse::Ok(value)
    }
}
