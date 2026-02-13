use interstice_sdk::*;
use std::f32::consts::TAU;

interstice_module!(visibility: Public, authorities: [Audio]);

const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
const INPUT_CHANNELS: u16 = 1;
const FRAMES_PER_BUFFER: u32 = 480;
const FREQUENCY: f32 = 440.0;
const VOLUME: f32 = 0.2;

// TABLES

#[table(ephemeral)]
#[derive(Clone)]
pub struct AudioState {
    #[primary_key]
    pub id: u64,
    pub output_stream_id: u64,
    pub input_stream_id: u64,
    pub phase: f32,
}

#[reducer(on = "load")]
fn on_load(ctx: ReducerContext) {
    let output_config = AudioStreamConfig {
        sample_rate: SAMPLE_RATE,
        channels: CHANNELS,
        frames_per_buffer: FRAMES_PER_BUFFER,
    };

    let output_stream_id = match ctx.audio().open_output_stream(output_config) {
        Ok(id) => id,
        Err(err) => {
            ctx.log(&format!("Audio output open failed: {err}"));
            0
        }
    };

    let input_config = AudioStreamConfig {
        sample_rate: SAMPLE_RATE,
        channels: INPUT_CHANNELS,
        frames_per_buffer: FRAMES_PER_BUFFER,
    };

    let input_stream_id = match ctx.audio().open_input_stream(input_config) {
        Ok(id) => id,
        Err(err) => {
            ctx.log(&format!("Audio input open failed: {err}"));
            0
        }
    };

    let row = AudioState {
        id: 0,
        output_stream_id,
        input_stream_id,
        phase: 0.0,
    };

    match ctx.current.tables.audiostate().insert(row.clone()) {
        Ok(_) => {}
        Err(err) => {
            ctx.log(&format!("Failed to init audio state: {err}"));
        }
    }

    ctx.log("Audio module loaded");
}

#[reducer(on = "audio_output")]
fn on_audio_output(ctx: ReducerContext) {
    let mut state = ctx.current.tables.audiostate().get(0).unwrap_or_else(|| {
        ctx.log("Audio state not found");
        AudioState {
            id: 0,
            output_stream_id: 0,
            input_stream_id: 0,
            phase: 0.0,
        }
    });

    let stream_id = state.output_stream_id;
    if stream_id == 0 {
        ctx.log("Audio output stream not initialized");
        return;
    }

    let frames = FRAMES_PER_BUFFER as usize;
    let channels = CHANNELS as usize;
    let mut data = vec![vec![0.0_f32; frames]; channels];

    let phase_step = TAU * FREQUENCY / SAMPLE_RATE as f32;
    for frame in 0..frames {
        let sample = state.phase.sin() * VOLUME;
        state.phase += phase_step;
        if state.phase >= TAU {
            state.phase -= TAU;
        }
        for channel in 0..channels {
            data[channel][frame] = sample;
        }
    }

    if let Err(err) =
        ctx.audio()
            .write_frames_f32_planar(stream_id, FRAMES_PER_BUFFER, CHANNELS, data)
    {
        ctx.log(&format!("Audio write failed: {err}"));
        return;
    }

    let _ = ctx.current.tables.audiostate().update(state);
}

#[reducer(on = "audio_input")]
fn on_audio_input(ctx: ReducerContext, stream_id: u64, data: Vec<Vec<f32>>) {
    let channel_count = data.len();
    ctx.log(&format!(
        "Audio input stream {} received {} channels",
        stream_id, channel_count
    ));
}
