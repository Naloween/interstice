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

    /// A key transition. `physical_key` is the key's position (layout
    /// independent); `logical_key` and `text` are what the host's keyboard
    /// layout made of it, when the host can tell (empty otherwise):
    /// - `logical_key`: a W3C `KeyboardEvent.key` value — the produced
    ///   character (`"a"`, `"é"`, `"&"`) or a key name (`"Enter"`,
    ///   `"ArrowLeft"`, `"Shift"`, `"Dead"`);
    /// - `text`: the text the press types (empty for releases and keys that
    ///   type nothing).
    Key {
        device_id: u32,
        physical_key: PhysicalKey,
        state: ElementState,
        logical_key: String,
        text: String,
        repeat: bool,
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
