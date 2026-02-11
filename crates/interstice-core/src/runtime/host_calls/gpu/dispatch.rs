use wasmtime::Caller;

use crate::{
    error::IntersticeError,
    runtime::{GpuCallRequest, Runtime, host_calls::gpu::GpuCallResult, wasm::StoreState},
};
use interstice_abi::GpuResponse;
use tokio::sync::oneshot;

impl Runtime {
    pub async fn handle_gpu_call(
        &self,
        call: interstice_abi::GpuCall,
        memory: &wasmtime::Memory,
        caller: &mut Caller<'_, StoreState>,
    ) -> Result<Option<i64>, IntersticeError> {
        let (tx, rx) = oneshot::channel();
        let _ = self.gpu_call_sender.send(GpuCallRequest {
            call,
            respond_to: tx,
        });

        let response = match rx.await {
            Ok(Ok(result)) => match result {
                GpuCallResult::None => GpuResponse::None,
                GpuCallResult::I64(v) => GpuResponse::I64(v),
                GpuCallResult::TextureFormat(format) => GpuResponse::TextureFormat(format),
            },
            Ok(Err(err)) => GpuResponse::Err(err.to_string()),
            Err(_) => GpuResponse::Err("Gpu call response dropped".into()),
        };

        let packed = self.send_data_to_module(response, memory, caller).await;
        Ok(Some(packed))
    }
}
