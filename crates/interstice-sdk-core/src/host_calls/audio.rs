use crate::host_calls::{host_call, unpack};
use interstice_abi::{AudioCall, AudioResponse, HostCall};

pub fn noop() -> Result<(), String> {
    let pack = host_call(HostCall::Audio(AudioCall::Noop));
    let response: AudioResponse = unpack(pack);
    match response {
        AudioResponse::Ok => Ok(()),
        AudioResponse::Err(err) => Err(err),
    }
}

pub struct Audio;

impl Audio {
    pub fn noop(&self) -> Result<(), String> {
        noop()
    }
}
