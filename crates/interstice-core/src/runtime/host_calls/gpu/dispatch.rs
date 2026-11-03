use wasmtime::Caller;

use crate::{
    error::IntersticeError,
    runtime::{GpuCallRequest, Runtime, host_calls::gpu::GpuCallResult, wasm::StoreState},
};
use interstice_abi::GpuResponse;

impl Runtime {
    pub fn handle_gpu_call(
        &self,
        call: interstice_abi::GpuCall,
        memory: &wasmtime::Memory,
        caller: &mut Caller<'_, StoreState>,
    ) -> Result<Option<i64>, IntersticeError> {
        let blob_id = match &call {
            interstice_abi::GpuCall::WriteTextureBlob(w) => Some(w.blob),
            interstice_abi::GpuCall::WriteBufferBlob(w) => Some(w.blob),
            _ => None,
        };
        let blob = match blob_id {
            Some(id) => match crate::runtime::blob::resolve_blob(id) {
                Some(b) => Some(b),
                None => {
                    let response = GpuResponse::Err(format!("unknown blob #{id}"));
                    return Ok(Some(self.send_data_to_module(response, memory, caller)));
                }
            },
            None => None,
        };

        let (tx, rx) = std::sync::mpsc::channel();
        let _ = self.gpu_call_sender.send(GpuCallRequest {
            call,
            blob,
            respond_to: tx,
        });

        let response = match rx.recv() {
            Ok(Ok(result)) => match result {
                GpuCallResult::None => GpuResponse::None,
                GpuCallResult::I64(v) => GpuResponse::I64(v),
                GpuCallResult::TextureFormat(format) => GpuResponse::TextureFormat(format),
                GpuCallResult::Extent2d { width, height } => {
                    GpuResponse::Extent2d { width, height }
                }
            },
            Ok(Err(err)) => GpuResponse::Err(err.to_string()),
            Err(_) => GpuResponse::Err("Gpu call response dropped".into()),
        };

        let packed = self.send_data_to_module(response, memory, caller);
        Ok(Some(packed))
    }
}
