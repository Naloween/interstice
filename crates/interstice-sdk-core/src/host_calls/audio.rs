use crate::host_calls::{host_call, unpack};
use interstice_abi::{AudioCall, AudioResponse, AudioStreamConfig, HostCall};

pub fn open_output_stream(config: AudioStreamConfig) -> Result<u64, String> {
    let pack = host_call(HostCall::Audio(AudioCall::OpenOutputStream(config)));
    let response: AudioResponse = unpack(pack);
    match response {
        AudioResponse::StreamOpened { stream_id } => Ok(stream_id),
        AudioResponse::Err(err) => Err(err),
        other => Err(format!("Unexpected audio response: {:?}", other)),
    }
}

pub fn open_input_stream(config: AudioStreamConfig) -> Result<u64, String> {
    let pack = host_call(HostCall::Audio(AudioCall::OpenInputStream(config)));
    let response: AudioResponse = unpack(pack);
    match response {
        AudioResponse::StreamOpened { stream_id } => Ok(stream_id),
        AudioResponse::Err(err) => Err(err),
        other => Err(format!("Unexpected audio response: {:?}", other)),
    }
}

pub fn close_stream(stream_id: u64) -> Result<(), String> {
    let pack = host_call(HostCall::Audio(AudioCall::CloseStream { stream_id }));
    let response: AudioResponse = unpack(pack);
    match response {
        AudioResponse::Ok => Ok(()),
        AudioResponse::Err(err) => Err(err),
        other => Err(format!("Unexpected audio response: {:?}", other)),
    }
}

pub fn write_frames_f32_planar(
    stream_id: u64,
    frames: u32,
    channels: u16,
    data: Vec<Vec<f32>>,
) -> Result<(), String> {
    let pack = host_call(HostCall::Audio(AudioCall::WriteFramesF32Planar {
        stream_id,
        frames,
        channels,
        data,
    }));
    let response: AudioResponse = unpack(pack);
    match response {
        AudioResponse::Ok => Ok(()),
        AudioResponse::Err(err) => Err(err),
        other => Err(format!("Unexpected audio response: {:?}", other)),
    }
}

pub struct Audio;

impl Audio {
    pub fn open_output_stream(&self, config: AudioStreamConfig) -> Result<u64, String> {
        open_output_stream(config)
    }

    pub fn close_stream(&self, stream_id: u64) -> Result<(), String> {
        close_stream(stream_id)
    }

    pub fn open_input_stream(&self, config: AudioStreamConfig) -> Result<u64, String> {
        open_input_stream(config)
    }

    pub fn write_frames_f32_planar(
        &self,
        stream_id: u64,
        frames: u32,
        channels: u16,
        data: Vec<Vec<f32>>,
    ) -> Result<(), String> {
        write_frames_f32_planar(stream_id, frames, channels, data)
    }
}
