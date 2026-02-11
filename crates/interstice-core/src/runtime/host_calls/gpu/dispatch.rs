use wasmtime::Caller;

use crate::{
    error::IntersticeError,
    runtime::{GpuCallRequest, Runtime, host_calls::gpu::GpuCallResult, wasm::StoreState},
};
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

        let result = rx
            .await
            .map_err(|_| IntersticeError::Internal("Gpu call response dropped".into()))??;

        match result {
            GpuCallResult::None => Ok(None),
            GpuCallResult::I64(v) => Ok(Some(v)),
            GpuCallResult::TextureFormat(format) => {
                let packed = self.send_data_to_module(format, memory, caller).await;
                Ok(Some(packed))
            }
        }
    }
}
