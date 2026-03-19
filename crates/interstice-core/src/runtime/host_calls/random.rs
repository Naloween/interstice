use crate::runtime::Runtime;
use crate::runtime::deterministic_random::next_u64;
use crate::runtime::reducer::CALL_STACK;
use interstice_abi::{DeterministicRandomRequest, DeterministicRandomResponse};

impl Runtime {
    pub(crate) fn handle_deterministic_random(
        &self,
        _request: DeterministicRandomRequest,
    ) -> DeterministicRandomResponse {
        CALL_STACK.with(|s| {
            match s.borrow_mut().last_mut() {
                Some(frame) => {
                    let value = next_u64(&mut frame.rng_state);
                    DeterministicRandomResponse::Ok(value)
                }
                None => DeterministicRandomResponse::Err(
                    "Deterministic random call outside of an active frame".into(),
                ),
            }
        })
    }
}
