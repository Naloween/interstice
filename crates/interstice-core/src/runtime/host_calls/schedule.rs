use crate::runtime::Runtime;
use interstice_abi::{IntersticeValue, ScheduleRequest, ScheduleResponse};

impl Runtime {
    pub(crate) fn handle_schedule(
        &self,
        module_name: String,
        request: ScheduleRequest,
    ) -> ScheduleResponse {
        let reducer_exists = {
            let modules = self.modules.lock().unwrap();
            let Some(module) = modules.get(&module_name) else {
                return ScheduleResponse::Err(format!("Module '{}' not found", module_name));
            };

            module
                .schema
                .reducers
                .iter()
                .find(|reducer| reducer.name == request.reducer_name)
                .map(|reducer| reducer.arguments.is_empty())
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

        let reducer_sender = self.reducer_sender.clone();
        let reducer_name = request.reducer_name;
        let caller_node_id = self.network_handle.node_id;

        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(request.delay_ms)).await;

            let _ = reducer_sender.send(crate::runtime::ReducerJob {
                module_name,
                reducer_name,
                input: IntersticeValue::Vec(vec![]),
                caller_node_id,
                completion: None,
            });
        });

        ScheduleResponse::Ok
    }
}
