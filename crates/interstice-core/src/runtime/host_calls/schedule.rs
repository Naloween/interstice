use crate::runtime::Runtime;
use crate::runtime::reducer::{CallFrameKind, OutboxEntry, CALL_STACK};
use interstice_abi::{ScheduleRequest, ScheduleResponse};

impl Runtime {
    pub(crate) fn handle_schedule(
        &self,
        module_name: String,
        request: ScheduleRequest,
    ) -> ScheduleResponse {
        let reducer_exists = CALL_STACK.with(|s| {
            let stack = s.borrow();
            let frame = match stack.last() {
                Some(f) => f,
                None => return Err("No active call frame".to_string()),
            };
            Ok(frame
                .module_arc
                .schema
                .reducers
                .iter()
                .find(|reducer| reducer.name == request.reducer_name)
                .map(|reducer| reducer.arguments.is_empty()))
        });
        let reducer_exists = match reducer_exists {
            Err(msg) => return ScheduleResponse::Err(msg),
            Ok(v) => v,
        };

        match reducer_exists {
            None => {
                return ScheduleResponse::Err(format!(
                    "Reducer '{}.{}' not found",
                    module_name, request.reducer_name
                ));
            }
            Some(false) => {
                return ScheduleResponse::Err(format!(
                    "Reducer '{}.{}' must have no arguments to be scheduled",
                    module_name, request.reducer_name
                ));
            }
            Some(true) => {}
        }

        // Released when the transaction commits (DESIGN §4.4).
        CALL_STACK.with(|s| match s.borrow_mut().first_mut() {
            Some(root) if root.kind == CallFrameKind::Reducer => {
                root.outbox.push(OutboxEntry::Schedule {
                    module_name,
                    reducer_name: request.reducer_name,
                    delay_ms: request.delay_ms,
                });
                ScheduleResponse::Ok
            }
            _ => ScheduleResponse::Err("Only reducers may schedule".into()),
        })
    }
}
