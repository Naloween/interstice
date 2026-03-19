use crate::runtime::AuthorityEntry;
use crate::runtime::event::EventInstance;
use crate::runtime::host_calls::audio::AudioState;
use interstice_abi::Authority;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;
use tokio::time::{Duration, sleep};

pub(crate) struct AudioEngine {
    audio_state: Arc<Mutex<AudioState>>,
    authority_modules: Arc<Mutex<HashMap<Authority, AuthorityEntry>>>,
    event_sender: UnboundedSender<(EventInstance, Option<crate::runtime::reducer::CompletionToken>)>,
}

impl AudioEngine {
    pub(crate) fn new(
        audio_state: Arc<Mutex<AudioState>>,
        authority_modules: Arc<Mutex<HashMap<Authority, AuthorityEntry>>>,
        event_sender: UnboundedSender<(EventInstance, Option<crate::runtime::reducer::CompletionToken>)>,
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
                    let audio_state = self.audio_state.lock();
                    audio_state.tick_interval()
                };

                if let Some(interval) = tick {
                    sleep(interval).await;

                    let has_audio_reducer = self
                        .authority_modules
                        .lock()
                        
                        .get(&Authority::Audio)
                        .and_then(|entry| match entry {
                            AuthorityEntry::Audio { output_reducer, .. } => output_reducer.as_ref(),
                            _ => None,
                        })
                        .is_some();

                    if has_audio_reducer {
                        let _ = self.event_sender.send((EventInstance::AudioOutput, None));
                    }

                    let input_event = {
                        let mut audio_state = self.audio_state.lock();
                        audio_state.take_input_frames()
                    };
                    if let Some((stream_id, data)) = input_event {
                        let _ = self
                            .event_sender
                            .send((EventInstance::AudioInput { stream_id, data }, None));
                    }
                } else {
                    sleep(Duration::from_millis(100)).await;
                }
            }
        });
    }
}
