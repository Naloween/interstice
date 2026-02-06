pub mod key_code;

use crate::interstice_abi_macros::IntersticeType;
use key_code::KeyCode;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, IntersticeType, Clone)]
pub enum InputEvent {
    Added {
        device_id: u32,
    },
    Removed {
        device_id: u32,
    },
    MouseMotion {
        device_id: u32,
        delta: (f64, f64),
    },
    MouseWheel {
        device_id: u32,
        delta: (f64, f64),
    },
    Motion {
        device_id: u32,
        axis_id: u32,
        value: f64,
    },
    Button {
        device_id: u32,
        button_id: u32,
        state: ElementState,
    },

    Key {
        device_id: u32,
        physical_key: PhysicalKey,
        state: ElementState,
    },
}

#[derive(Debug, Deserialize, Serialize, IntersticeType, Clone)]
pub enum ElementState {
    Pressed,
    Released,
}

#[derive(Debug, Deserialize, Serialize, IntersticeType, Clone)]
pub enum PhysicalKey {
    /// A known key code
    Code(KeyCode),
    /// This variant is used when the key cannot be translated to a [`KeyCode`]
    Unidentified(u32),
}
