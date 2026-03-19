use crate::runtime::Runtime;
use crate::runtime::reducer::CALL_STACK;
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

        let reducer_name = request.reducer_name;
        let wake_at = std::time::Instant::now() + std::time::Duration::from_millis(request.delay_ms);
        let _ = self.timer_tx.send((wake_at, module_name, reducer_name));

        ScheduleResponse::Ok
    }
}
