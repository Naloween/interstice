use crate::runtime::AuthorityEntry;
use crate::runtime::event::EventInstance;
use crate::runtime::host_calls::audio::AudioState;
use interstice_abi::Authority;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::UnboundedSender;
use tokio::time::{Duration, sleep};

pub(crate) struct AudioEngine {
    audio_state: Arc<Mutex<AudioState>>,
    authority_modules: Arc<Mutex<HashMap<Authority, AuthorityEntry>>>,
    event_sender: UnboundedSender<EventInstance>,
}

impl AudioEngine {
    pub(crate) fn new(
        audio_state: Arc<Mutex<AudioState>>,
        authority_modules: Arc<Mutex<HashMap<Authority, AuthorityEntry>>>,
        event_sender: UnboundedSender<EventInstance>,
    ) -> Self {
        Self {
            audio_state,
            authority_modules,
            event_sender,
        }
    }

    pub(crate) fn spawn(self) {
        tokio::task::spawn_local(async move {
            loop {
                let tick = {
                    let audio_state = self.audio_state.lock().unwrap();
                    audio_state.tick_interval()
                };

                if let Some(interval) = tick {
                    sleep(interval).await;

                    let has_audio_reducer = self
                        .authority_modules
                        .lock()
                        .unwrap()
                        .get(&Authority::Audio)
                        .and_then(|entry| entry.on_event_reducer_name.as_ref())
                        .is_some();

                    if has_audio_reducer {
                        let _ = self.event_sender.send(EventInstance::AudioOutput);
                    }

                    let input_event = {
                        let mut audio_state = self.audio_state.lock().unwrap();
                        audio_state.take_input_frames()
                    };
                    if let Some((stream_id, data)) = input_event {
                        let _ = self
                            .event_sender
                            .send(EventInstance::AudioInput { stream_id, data });
                    }
                } else {
                    sleep(Duration::from_millis(100)).await;
                }
            }
        });
    }
}
