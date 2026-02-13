use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AudioStreamConfig {
    pub sample_rate: u32,
    pub channels: u16,
    pub frames_per_buffer: u32,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum AudioCall {
    OpenOutputStream(AudioStreamConfig),
    OpenInputStream(AudioStreamConfig),
    CloseStream {
        stream_id: u64,
    },
    WriteFramesF32Planar {
        stream_id: u64,
        frames: u32,
        channels: u16,
        data: Vec<Vec<f32>>,
    },
}

#[derive(Debug, Deserialize, Serialize)]
pub enum AudioResponse {
    Ok,
    StreamOpened { stream_id: u64 },
    Err(String),
}
