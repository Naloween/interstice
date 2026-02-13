use crate::runtime::Runtime;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, SampleFormat, SampleRate, StreamConfig};
use interstice_abi::{AudioCall, AudioResponse, AudioStreamConfig};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

pub(crate) struct AudioStreamState {
    pub config: AudioStreamConfig,
    buffer: Arc<Mutex<VecDeque<f32>>>,
}

pub(crate) struct AudioInputState {
    pub config: AudioStreamConfig,
    buffer: Arc<Mutex<VecDeque<f32>>>,
}

pub(crate) struct AudioState {
    next_stream_id: u64,
    output_streams: HashMap<u64, AudioStreamState>,
    input_streams: HashMap<u64, AudioInputState>,
    command_sender: mpsc::Sender<AudioCommand>,
}

impl AudioState {
    pub(crate) fn new(command_sender: mpsc::Sender<AudioCommand>) -> Self {
        Self {
            next_stream_id: 1,
            output_streams: HashMap::new(),
            input_streams: HashMap::new(),
            command_sender,
        }
    }

    pub(crate) fn tick_interval(&self) -> Option<Duration> {
        let config = self
            .output_streams
            .values()
            .next()
            .map(|stream| stream.config.clone())
            .or_else(|| {
                self.input_streams
                    .values()
                    .next()
                    .map(|stream| stream.config.clone())
            })?;
        let sample_rate = config.sample_rate.max(1) as f64;
        let frames = config.frames_per_buffer.max(1) as f64;
        let seconds = frames / sample_rate;
        Some(Duration::from_secs_f64(seconds))
    }

    pub(crate) fn take_input_frames(&mut self) -> Option<(u64, Vec<Vec<f32>>)> {
        let (stream_id, stream) = self.input_streams.iter().find(|(_, stream)| {
            let needed = stream.config.frames_per_buffer as usize * stream.config.channels as usize;
            let buffer = stream.buffer.lock().unwrap();
            buffer.len() >= needed
        })?;

        let frames = stream.config.frames_per_buffer as usize;
        let channels = stream.config.channels as usize;
        let samples_needed = frames * channels;
        let mut buffer = stream.buffer.lock().unwrap();
        if buffer.len() < samples_needed {
            return None;
        }

        let mut interleaved = Vec::with_capacity(samples_needed);
        for _ in 0..samples_needed {
            if let Some(sample) = buffer.pop_front() {
                interleaved.push(sample);
            }
        }

        let mut planar = vec![vec![0.0_f32; frames]; channels];
        for frame in 0..frames {
            for channel in 0..channels {
                planar[channel][frame] = interleaved[frame * channels + channel];
            }
        }

        Some((*stream_id, planar))
    }
}

pub enum AudioCommand {
    OpenOutput {
        stream_id: u64,
        config: AudioStreamConfig,
        respond_to: mpsc::Sender<Result<Arc<Mutex<VecDeque<f32>>>, String>>,
    },
    OpenInput {
        stream_id: u64,
        config: AudioStreamConfig,
        respond_to: mpsc::Sender<Result<Arc<Mutex<VecDeque<f32>>>, String>>,
    },
    Close {
        stream_id: u64,
    },
}

pub(crate) fn start_audio_thread() -> mpsc::Sender<AudioCommand> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || audio_thread(receiver));
    sender
}

fn audio_thread(receiver: mpsc::Receiver<AudioCommand>) {
    let mut output_streams: HashMap<u64, cpal::Stream> = HashMap::new();
    let mut input_streams: HashMap<u64, cpal::Stream> = HashMap::new();

    while let Ok(command) = receiver.recv() {
        match command {
            AudioCommand::OpenOutput {
                stream_id,
                config,
                respond_to,
            } => {
                let result = build_output_stream(&config).map(|(stream, buffer)| {
                    output_streams.insert(stream_id, stream);
                    buffer
                });
                let _ = respond_to.send(result);
            }
            AudioCommand::OpenInput {
                stream_id,
                config,
                respond_to,
            } => {
                let result = build_input_stream(&config).map(|(stream, buffer)| {
                    input_streams.insert(stream_id, stream);
                    buffer
                });
                let _ = respond_to.send(result);
            }
            AudioCommand::Close { stream_id } => {
                output_streams.remove(&stream_id);
                input_streams.remove(&stream_id);
            }
        }
    }
}

fn build_output_stream(
    config: &AudioStreamConfig,
) -> Result<(cpal::Stream, Arc<Mutex<VecDeque<f32>>>), String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| "No audio output device available".to_string())?;

    let mut chosen = None;
    let supported_configs = device
        .supported_output_configs()
        .map_err(|err| format!("Failed to query output configs: {err}"))?;
    for supported in supported_configs {
        if supported.sample_format() != SampleFormat::F32 {
            continue;
        }
        if supported.channels() != config.channels {
            continue;
        }
        let min = supported.min_sample_rate().0;
        let max = supported.max_sample_rate().0;
        if config.sample_rate < min || config.sample_rate > max {
            continue;
        }
        chosen = Some(supported.with_sample_rate(SampleRate(config.sample_rate)));
        break;
    }

    let chosen = chosen.ok_or_else(|| {
        format!(
            "No supported f32 output config for {} channels at {} Hz",
            config.channels, config.sample_rate
        )
    })?;

    let stream_config = StreamConfig {
        channels: chosen.channels(),
        sample_rate: chosen.sample_rate(),
        buffer_size: BufferSize::Fixed(config.frames_per_buffer),
    };

    let buffer = Arc::new(Mutex::new(VecDeque::new()));
    let buffer_for_callback = buffer.clone();

    let stream = device
        .build_output_stream(
            &stream_config,
            move |data: &mut [f32], _info| {
                let mut guard = buffer_for_callback.lock().unwrap();
                for sample in data.iter_mut() {
                    *sample = guard.pop_front().unwrap_or(0.0);
                }
            },
            move |err| {
                eprintln!("Audio stream error: {err}");
            },
            None,
        )
        .map_err(|err| format!("Failed to build output stream: {err}"))?;

    stream
        .play()
        .map_err(|err| format!("Failed to start output stream: {err}"))?;

    Ok((stream, buffer))
}

fn build_input_stream(
    config: &AudioStreamConfig,
) -> Result<(cpal::Stream, Arc<Mutex<VecDeque<f32>>>), String> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| "No audio input device available".to_string())?;

    let mut chosen = None;
    let supported_configs = device
        .supported_input_configs()
        .map_err(|err| format!("Failed to query input configs: {err}"))?;
    for supported in supported_configs {
        if supported.sample_format() != SampleFormat::F32 {
            continue;
        }
        if supported.channels() != config.channels {
            continue;
        }
        let min = supported.min_sample_rate().0;
        let max = supported.max_sample_rate().0;
        if config.sample_rate < min || config.sample_rate > max {
            continue;
        }
        chosen = Some(supported.with_sample_rate(SampleRate(config.sample_rate)));
        break;
    }

    let chosen = chosen.ok_or_else(|| {
        format!(
            "No supported f32 input config for {} channels at {} Hz",
            config.channels, config.sample_rate
        )
    })?;

    let stream_config = StreamConfig {
        channels: chosen.channels(),
        sample_rate: chosen.sample_rate(),
        buffer_size: BufferSize::Fixed(config.frames_per_buffer),
    };

    let buffer = Arc::new(Mutex::new(VecDeque::new()));
    let buffer_for_callback = buffer.clone();
    let max_samples = (config.frames_per_buffer as usize)
        .saturating_mul(config.channels as usize)
        .saturating_mul(4);

    let stream = device
        .build_input_stream(
            &stream_config,
            move |data: &[f32], _info| {
                let mut guard = buffer_for_callback.lock().unwrap();
                for sample in data.iter() {
                    guard.push_back(*sample);
                }
                while guard.len() > max_samples {
                    let _ = guard.pop_front();
                }
            },
            move |err| {
                eprintln!("Audio input stream error: {err}");
            },
            None,
        )
        .map_err(|err| format!("Failed to build input stream: {err}"))?;

    stream
        .play()
        .map_err(|err| format!("Failed to start input stream: {err}"))?;

    Ok((stream, buffer))
}

impl Runtime {
    pub(crate) fn handle_audio_call(&self, call: AudioCall) -> AudioResponse {
        let mut audio_state = self.audio_state.lock().unwrap();

        match call {
            AudioCall::OpenOutputStream(config) => {
                if config.sample_rate == 0 || config.channels == 0 || config.frames_per_buffer == 0
                {
                    return AudioResponse::Err("Invalid audio stream config".into());
                }

                let stream_id = audio_state.next_stream_id;
                audio_state.next_stream_id = audio_state.next_stream_id.wrapping_add(1);
                let command_sender = audio_state.command_sender.clone();
                drop(audio_state);

                let (respond_to, response) = mpsc::channel();
                if command_sender
                    .send(AudioCommand::OpenOutput {
                        stream_id,
                        config: config.clone(),
                        respond_to,
                    })
                    .is_err()
                {
                    return AudioResponse::Err("Audio thread unavailable".into());
                }

                let buffer = match response.recv() {
                    Ok(Ok(buffer)) => buffer,
                    Ok(Err(err)) => return AudioResponse::Err(err),
                    Err(_) => return AudioResponse::Err("Audio thread unavailable".into()),
                };

                let mut audio_state = self.audio_state.lock().unwrap();
                audio_state.output_streams.insert(
                    stream_id,
                    AudioStreamState {
                        config: config.clone(),
                        buffer,
                    },
                );
                AudioResponse::StreamOpened { stream_id }
            }
            AudioCall::OpenInputStream(config) => {
                if config.sample_rate == 0 || config.channels == 0 || config.frames_per_buffer == 0
                {
                    return AudioResponse::Err("Invalid audio stream config".into());
                }

                let stream_id = audio_state.next_stream_id;
                audio_state.next_stream_id = audio_state.next_stream_id.wrapping_add(1);
                let command_sender = audio_state.command_sender.clone();
                drop(audio_state);

                let (respond_to, response) = mpsc::channel();
                if command_sender
                    .send(AudioCommand::OpenInput {
                        stream_id,
                        config: config.clone(),
                        respond_to,
                    })
                    .is_err()
                {
                    return AudioResponse::Err("Audio thread unavailable".into());
                }

                let buffer = match response.recv() {
                    Ok(Ok(buffer)) => buffer,
                    Ok(Err(err)) => return AudioResponse::Err(err),
                    Err(_) => return AudioResponse::Err("Audio thread unavailable".into()),
                };

                let mut audio_state = self.audio_state.lock().unwrap();
                audio_state.input_streams.insert(
                    stream_id,
                    AudioInputState {
                        config: config.clone(),
                        buffer,
                    },
                );
                AudioResponse::StreamOpened { stream_id }
            }
            AudioCall::CloseStream { stream_id } => {
                let removed_output = audio_state.output_streams.remove(&stream_id).is_some();
                let removed_input = audio_state.input_streams.remove(&stream_id).is_some();
                let command_sender = audio_state.command_sender.clone();
                drop(audio_state);

                let _ = command_sender.send(AudioCommand::Close { stream_id });
                if removed_output || removed_input {
                    AudioResponse::Ok
                } else {
                    AudioResponse::Err("Audio stream not found".into())
                }
            }
            AudioCall::WriteFramesF32Planar {
                stream_id,
                frames,
                channels,
                data,
            } => {
                let (config, buffer) = match audio_state.output_streams.get(&stream_id) {
                    Some(stream) => (stream.config.clone(), stream.buffer.clone()),
                    None => return AudioResponse::Err("Audio stream not found".into()),
                };
                drop(audio_state);

                if channels != config.channels {
                    return AudioResponse::Err("Channel count mismatch".into());
                }

                if frames != config.frames_per_buffer {
                    return AudioResponse::Err("Frame count mismatch".into());
                }

                if data.len() != channels as usize {
                    return AudioResponse::Err("Planar channel data mismatch".into());
                }

                if data.iter().any(|channel| channel.len() != frames as usize) {
                    return AudioResponse::Err("Planar frame data mismatch".into());
                }

                let frames = frames as usize;
                let channels = channels as usize;
                let mut interleaved = Vec::with_capacity(frames * channels);
                for frame in 0..frames {
                    for channel in 0..channels {
                        interleaved.push(data[channel][frame]);
                    }
                }

                let max_samples = (config.frames_per_buffer as usize)
                    .saturating_mul(config.channels as usize)
                    .saturating_mul(4);

                let mut buffer = buffer.lock().unwrap();
                if buffer.len() + interleaved.len() > max_samples {
                    return AudioResponse::Err("Audio buffer overflow".into());
                }
                buffer.extend(interleaved);

                AudioResponse::Ok
            }
        }
    }
}
