use crate::runtime::Runtime;
use interstice_abi::{AudioCall, AudioResponse};

impl Runtime {
	pub(crate) fn handle_audio_call(&self, call: AudioCall) -> AudioResponse {
		match call {
			AudioCall::Noop => AudioResponse::Ok,
		}
	}
}
