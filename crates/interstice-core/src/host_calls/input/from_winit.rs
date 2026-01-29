use interstice_abi::{ElementState, InputEvent, PhysicalKey, key_code::KeyCode};
use winit::event::{DeviceEvent, MouseScrollDelta};

pub fn get_input_event_from_device_event(device_id: u32, event: DeviceEvent) -> InputEvent {
    return match event {
        DeviceEvent::Added => InputEvent::Added { device_id },
        DeviceEvent::Removed => InputEvent::Removed { device_id },
        DeviceEvent::MouseMotion { delta } => InputEvent::MouseMotion { device_id, delta },
        DeviceEvent::MouseWheel { delta } => {
            let delta = match delta {
                MouseScrollDelta::LineDelta(a, b) => (a as f64, b as f64),
                MouseScrollDelta::PixelDelta(p) => (p.x, p.y),
            };
            InputEvent::MouseWheel { device_id, delta }
        }
        DeviceEvent::Motion { axis, value } => InputEvent::Motion {
            device_id,
            axis_id: axis,
            value,
        },
        DeviceEvent::Button { button, state } => InputEvent::Button {
            device_id,
            button_id: button,
            state: match state {
                winit::event::ElementState::Pressed => ElementState::Pressed,
                winit::event::ElementState::Released => ElementState::Released,
            },
        },
        DeviceEvent::Key(raw_key_event) => InputEvent::Key {
            device_id,
            physical_key: match raw_key_event.physical_key {
                winit::keyboard::PhysicalKey::Code(key_code) => match key_code {
                    winit::keyboard::KeyCode::Backquote => PhysicalKey::Code(KeyCode::Backquote),
                    winit::keyboard::KeyCode::Backslash => PhysicalKey::Code(KeyCode::Backslash),
                    winit::keyboard::KeyCode::BracketLeft => {
                        PhysicalKey::Code(KeyCode::BracketLeft)
                    }
                    winit::keyboard::KeyCode::BracketRight => {
                        PhysicalKey::Code(KeyCode::BracketRight)
                    }
                    winit::keyboard::KeyCode::Comma => PhysicalKey::Code(KeyCode::Comma),
                    winit::keyboard::KeyCode::Digit0 => PhysicalKey::Code(KeyCode::Digit0),
                    winit::keyboard::KeyCode::Digit1 => PhysicalKey::Code(KeyCode::Digit1),
                    winit::keyboard::KeyCode::Digit2 => PhysicalKey::Code(KeyCode::Digit2),
                    winit::keyboard::KeyCode::Digit3 => PhysicalKey::Code(KeyCode::Digit3),
                    winit::keyboard::KeyCode::Digit4 => PhysicalKey::Code(KeyCode::Digit4),
                    winit::keyboard::KeyCode::Digit5 => PhysicalKey::Code(KeyCode::Digit5),
                    winit::keyboard::KeyCode::Digit6 => PhysicalKey::Code(KeyCode::Digit6),
                    winit::keyboard::KeyCode::Digit7 => PhysicalKey::Code(KeyCode::Digit7),
                    winit::keyboard::KeyCode::Digit8 => PhysicalKey::Code(KeyCode::Digit8),
                    winit::keyboard::KeyCode::Digit9 => PhysicalKey::Code(KeyCode::Digit9),
                    winit::keyboard::KeyCode::Equal => PhysicalKey::Code(KeyCode::Equal),
                    winit::keyboard::KeyCode::IntlBackslash => {
                        PhysicalKey::Code(KeyCode::IntlBackslash)
                    }
                    winit::keyboard::KeyCode::IntlRo => PhysicalKey::Code(KeyCode::IntlRo),
                    winit::keyboard::KeyCode::IntlYen => PhysicalKey::Code(KeyCode::IntlYen),
                    winit::keyboard::KeyCode::KeyA => PhysicalKey::Code(KeyCode::KeyA),
                    winit::keyboard::KeyCode::KeyB => PhysicalKey::Code(KeyCode::KeyB),
                    winit::keyboard::KeyCode::KeyC => PhysicalKey::Code(KeyCode::KeyC),
                    winit::keyboard::KeyCode::KeyD => PhysicalKey::Code(KeyCode::KeyD),
                    winit::keyboard::KeyCode::KeyE => PhysicalKey::Code(KeyCode::KeyE),
                    winit::keyboard::KeyCode::KeyF => PhysicalKey::Code(KeyCode::KeyF),
                    winit::keyboard::KeyCode::KeyG => PhysicalKey::Code(KeyCode::KeyG),
                    winit::keyboard::KeyCode::KeyH => PhysicalKey::Code(KeyCode::KeyH),
                    winit::keyboard::KeyCode::KeyI => PhysicalKey::Code(KeyCode::KeyI),
                    winit::keyboard::KeyCode::KeyJ => PhysicalKey::Code(KeyCode::KeyJ),
                    winit::keyboard::KeyCode::KeyK => PhysicalKey::Code(KeyCode::KeyK),
                    winit::keyboard::KeyCode::KeyL => PhysicalKey::Code(KeyCode::KeyL),
                    winit::keyboard::KeyCode::KeyM => PhysicalKey::Code(KeyCode::KeyM),
                    winit::keyboard::KeyCode::KeyN => PhysicalKey::Code(KeyCode::KeyN),
                    winit::keyboard::KeyCode::KeyO => PhysicalKey::Code(KeyCode::KeyO),
                    winit::keyboard::KeyCode::KeyP => PhysicalKey::Code(KeyCode::KeyP),
                    winit::keyboard::KeyCode::KeyQ => PhysicalKey::Code(KeyCode::KeyQ),
                    winit::keyboard::KeyCode::KeyR => PhysicalKey::Code(KeyCode::KeyR),
                    winit::keyboard::KeyCode::KeyS => PhysicalKey::Code(KeyCode::KeyS),
                    winit::keyboard::KeyCode::KeyT => PhysicalKey::Code(KeyCode::KeyT),
                    winit::keyboard::KeyCode::KeyU => PhysicalKey::Code(KeyCode::KeyU),
                    winit::keyboard::KeyCode::KeyV => PhysicalKey::Code(KeyCode::KeyV),
                    winit::keyboard::KeyCode::KeyW => PhysicalKey::Code(KeyCode::KeyW),
                    winit::keyboard::KeyCode::KeyX => PhysicalKey::Code(KeyCode::KeyX),
                    winit::keyboard::KeyCode::KeyY => PhysicalKey::Code(KeyCode::KeyY),
                    winit::keyboard::KeyCode::KeyZ => PhysicalKey::Code(KeyCode::KeyZ),
                    winit::keyboard::KeyCode::Minus => PhysicalKey::Code(KeyCode::Minus),
                    winit::keyboard::KeyCode::Period => PhysicalKey::Code(KeyCode::Period),
                    winit::keyboard::KeyCode::Quote => PhysicalKey::Code(KeyCode::Quote),
                    winit::keyboard::KeyCode::Semicolon => PhysicalKey::Code(KeyCode::Semicolon),
                    winit::keyboard::KeyCode::Slash => PhysicalKey::Code(KeyCode::Slash),
                    winit::keyboard::KeyCode::AltLeft => PhysicalKey::Code(KeyCode::AltLeft),
                    winit::keyboard::KeyCode::AltRight => PhysicalKey::Code(KeyCode::AltRight),
                    winit::keyboard::KeyCode::Backspace => PhysicalKey::Code(KeyCode::Backspace),
                    winit::keyboard::KeyCode::CapsLock => PhysicalKey::Code(KeyCode::CapsLock),
                    winit::keyboard::KeyCode::ContextMenu => {
                        PhysicalKey::Code(KeyCode::ContextMenu)
                    }
                    winit::keyboard::KeyCode::ControlLeft => {
                        PhysicalKey::Code(KeyCode::ControlLeft)
                    }
                    winit::keyboard::KeyCode::ControlRight => {
                        PhysicalKey::Code(KeyCode::ControlRight)
                    }
                    winit::keyboard::KeyCode::Enter => PhysicalKey::Code(KeyCode::Enter),
                    winit::keyboard::KeyCode::SuperLeft => PhysicalKey::Code(KeyCode::SuperLeft),
                    winit::keyboard::KeyCode::SuperRight => PhysicalKey::Code(KeyCode::SuperRight),
                    winit::keyboard::KeyCode::ShiftLeft => PhysicalKey::Code(KeyCode::ShiftLeft),
                    winit::keyboard::KeyCode::ShiftRight => PhysicalKey::Code(KeyCode::ShiftRight),
                    winit::keyboard::KeyCode::Space => PhysicalKey::Code(KeyCode::Space),
                    winit::keyboard::KeyCode::Tab => PhysicalKey::Code(KeyCode::Tab),
                    winit::keyboard::KeyCode::Convert => PhysicalKey::Code(KeyCode::Convert),
                    winit::keyboard::KeyCode::KanaMode => PhysicalKey::Code(KeyCode::KanaMode),
                    winit::keyboard::KeyCode::Lang1 => PhysicalKey::Code(KeyCode::Lang1),
                    winit::keyboard::KeyCode::Lang2 => PhysicalKey::Code(KeyCode::Lang2),
                    winit::keyboard::KeyCode::Lang3 => PhysicalKey::Code(KeyCode::Lang3),
                    winit::keyboard::KeyCode::Lang4 => PhysicalKey::Code(KeyCode::Lang4),
                    winit::keyboard::KeyCode::Lang5 => PhysicalKey::Code(KeyCode::Lang5),
                    winit::keyboard::KeyCode::NonConvert => PhysicalKey::Code(KeyCode::NonConvert),
                    winit::keyboard::KeyCode::Delete => PhysicalKey::Code(KeyCode::Delete),
                    winit::keyboard::KeyCode::End => PhysicalKey::Code(KeyCode::End),
                    winit::keyboard::KeyCode::Help => PhysicalKey::Code(KeyCode::Help),
                    winit::keyboard::KeyCode::Home => PhysicalKey::Code(KeyCode::Home),
                    winit::keyboard::KeyCode::Insert => PhysicalKey::Code(KeyCode::Insert),
                    winit::keyboard::KeyCode::PageDown => PhysicalKey::Code(KeyCode::PageDown),
                    winit::keyboard::KeyCode::PageUp => PhysicalKey::Code(KeyCode::PageUp),
                    winit::keyboard::KeyCode::ArrowDown => PhysicalKey::Code(KeyCode::ArrowDown),
                    winit::keyboard::KeyCode::ArrowLeft => PhysicalKey::Code(KeyCode::ArrowLeft),
                    winit::keyboard::KeyCode::ArrowRight => PhysicalKey::Code(KeyCode::ArrowRight),
                    winit::keyboard::KeyCode::ArrowUp => PhysicalKey::Code(KeyCode::ArrowUp),
                    winit::keyboard::KeyCode::NumLock => PhysicalKey::Code(KeyCode::NumLock),
                    winit::keyboard::KeyCode::Numpad0 => PhysicalKey::Code(KeyCode::Numpad0),
                    winit::keyboard::KeyCode::Numpad1 => PhysicalKey::Code(KeyCode::Numpad1),
                    winit::keyboard::KeyCode::Numpad2 => PhysicalKey::Code(KeyCode::Numpad2),
                    winit::keyboard::KeyCode::Numpad3 => PhysicalKey::Code(KeyCode::Numpad3),
                    winit::keyboard::KeyCode::Numpad4 => PhysicalKey::Code(KeyCode::Numpad4),
                    winit::keyboard::KeyCode::Numpad5 => PhysicalKey::Code(KeyCode::Numpad5),
                    winit::keyboard::KeyCode::Numpad6 => PhysicalKey::Code(KeyCode::Numpad6),
                    winit::keyboard::KeyCode::Numpad7 => PhysicalKey::Code(KeyCode::Numpad7),
                    winit::keyboard::KeyCode::Numpad8 => PhysicalKey::Code(KeyCode::Numpad8),
                    winit::keyboard::KeyCode::Numpad9 => PhysicalKey::Code(KeyCode::Numpad9),
                    winit::keyboard::KeyCode::NumpadAdd => PhysicalKey::Code(KeyCode::NumpadAdd),
                    winit::keyboard::KeyCode::NumpadBackspace => {
                        PhysicalKey::Code(KeyCode::NumpadBackspace)
                    }
                    winit::keyboard::KeyCode::NumpadClear => {
                        PhysicalKey::Code(KeyCode::NumpadClear)
                    }
                    winit::keyboard::KeyCode::NumpadClearEntry => {
                        PhysicalKey::Code(KeyCode::NumpadClearEntry)
                    }
                    winit::keyboard::KeyCode::NumpadComma => {
                        PhysicalKey::Code(KeyCode::NumpadComma)
                    }
                    winit::keyboard::KeyCode::NumpadDecimal => {
                        PhysicalKey::Code(KeyCode::NumpadDecimal)
                    }
                    winit::keyboard::KeyCode::NumpadDivide => {
                        PhysicalKey::Code(KeyCode::NumpadDivide)
                    }
                    winit::keyboard::KeyCode::NumpadEnter => {
                        PhysicalKey::Code(KeyCode::NumpadEnter)
                    }
                    winit::keyboard::KeyCode::NumpadEqual => {
                        PhysicalKey::Code(KeyCode::NumpadEqual)
                    }
                    winit::keyboard::KeyCode::NumpadHash => PhysicalKey::Code(KeyCode::NumpadHash),
                    winit::keyboard::KeyCode::NumpadMemoryAdd => {
                        PhysicalKey::Code(KeyCode::NumpadMemoryAdd)
                    }
                    winit::keyboard::KeyCode::NumpadMemoryClear => {
                        PhysicalKey::Code(KeyCode::NumpadMemoryClear)
                    }
                    winit::keyboard::KeyCode::NumpadMemoryRecall => {
                        PhysicalKey::Code(KeyCode::NumpadMemoryRecall)
                    }
                    winit::keyboard::KeyCode::NumpadMemoryStore => {
                        PhysicalKey::Code(KeyCode::NumpadMemoryStore)
                    }
                    winit::keyboard::KeyCode::NumpadMemorySubtract => {
                        PhysicalKey::Code(KeyCode::NumpadMemorySubtract)
                    }
                    winit::keyboard::KeyCode::NumpadMultiply => {
                        PhysicalKey::Code(KeyCode::NumpadMultiply)
                    }
                    winit::keyboard::KeyCode::NumpadParenLeft => {
                        PhysicalKey::Code(KeyCode::NumpadParenLeft)
                    }
                    winit::keyboard::KeyCode::NumpadParenRight => {
                        PhysicalKey::Code(KeyCode::NumpadParenRight)
                    }
                    winit::keyboard::KeyCode::NumpadStar => PhysicalKey::Code(KeyCode::NumpadStar),
                    winit::keyboard::KeyCode::NumpadSubtract => {
                        PhysicalKey::Code(KeyCode::NumpadSubtract)
                    }
                    winit::keyboard::KeyCode::Escape => PhysicalKey::Code(KeyCode::Escape),
                    winit::keyboard::KeyCode::Fn => PhysicalKey::Code(KeyCode::Fn),
                    winit::keyboard::KeyCode::FnLock => PhysicalKey::Code(KeyCode::FnLock),
                    winit::keyboard::KeyCode::PrintScreen => {
                        PhysicalKey::Code(KeyCode::PrintScreen)
                    }
                    winit::keyboard::KeyCode::ScrollLock => PhysicalKey::Code(KeyCode::ScrollLock),
                    winit::keyboard::KeyCode::Pause => PhysicalKey::Code(KeyCode::Pause),
                    winit::keyboard::KeyCode::BrowserBack => {
                        PhysicalKey::Code(KeyCode::BrowserBack)
                    }
                    winit::keyboard::KeyCode::BrowserFavorites => {
                        PhysicalKey::Code(KeyCode::BrowserFavorites)
                    }
                    winit::keyboard::KeyCode::BrowserForward => {
                        PhysicalKey::Code(KeyCode::BrowserForward)
                    }
                    winit::keyboard::KeyCode::BrowserHome => {
                        PhysicalKey::Code(KeyCode::BrowserHome)
                    }
                    winit::keyboard::KeyCode::BrowserRefresh => {
                        PhysicalKey::Code(KeyCode::BrowserRefresh)
                    }
                    winit::keyboard::KeyCode::BrowserSearch => {
                        PhysicalKey::Code(KeyCode::BrowserSearch)
                    }
                    winit::keyboard::KeyCode::BrowserStop => {
                        PhysicalKey::Code(KeyCode::BrowserStop)
                    }
                    winit::keyboard::KeyCode::Eject => PhysicalKey::Code(KeyCode::Eject),
                    winit::keyboard::KeyCode::LaunchApp1 => PhysicalKey::Code(KeyCode::LaunchApp1),
                    winit::keyboard::KeyCode::LaunchApp2 => PhysicalKey::Code(KeyCode::LaunchApp2),
                    winit::keyboard::KeyCode::LaunchMail => PhysicalKey::Code(KeyCode::LaunchMail),
                    winit::keyboard::KeyCode::MediaPlayPause => {
                        PhysicalKey::Code(KeyCode::MediaPlayPause)
                    }
                    winit::keyboard::KeyCode::MediaSelect => {
                        PhysicalKey::Code(KeyCode::MediaSelect)
                    }
                    winit::keyboard::KeyCode::MediaStop => PhysicalKey::Code(KeyCode::MediaStop),
                    winit::keyboard::KeyCode::MediaTrackNext => {
                        PhysicalKey::Code(KeyCode::MediaTrackNext)
                    }
                    winit::keyboard::KeyCode::MediaTrackPrevious => {
                        PhysicalKey::Code(KeyCode::MediaTrackPrevious)
                    }
                    winit::keyboard::KeyCode::Power => PhysicalKey::Code(KeyCode::Power),
                    winit::keyboard::KeyCode::Sleep => PhysicalKey::Code(KeyCode::Sleep),
                    winit::keyboard::KeyCode::AudioVolumeDown => {
                        PhysicalKey::Code(KeyCode::AudioVolumeDown)
                    }
                    winit::keyboard::KeyCode::AudioVolumeMute => {
                        PhysicalKey::Code(KeyCode::AudioVolumeMute)
                    }
                    winit::keyboard::KeyCode::AudioVolumeUp => {
                        PhysicalKey::Code(KeyCode::AudioVolumeUp)
                    }
                    winit::keyboard::KeyCode::WakeUp => PhysicalKey::Code(KeyCode::WakeUp),
                    winit::keyboard::KeyCode::Meta => PhysicalKey::Code(KeyCode::Meta),
                    winit::keyboard::KeyCode::Hyper => PhysicalKey::Code(KeyCode::Hyper),
                    winit::keyboard::KeyCode::Turbo => PhysicalKey::Code(KeyCode::Turbo),
                    winit::keyboard::KeyCode::Abort => PhysicalKey::Code(KeyCode::Abort),
                    winit::keyboard::KeyCode::Resume => PhysicalKey::Code(KeyCode::Resume),
                    winit::keyboard::KeyCode::Suspend => PhysicalKey::Code(KeyCode::Suspend),
                    winit::keyboard::KeyCode::Again => PhysicalKey::Code(KeyCode::Again),
                    winit::keyboard::KeyCode::Copy => PhysicalKey::Code(KeyCode::Copy),
                    winit::keyboard::KeyCode::Cut => PhysicalKey::Code(KeyCode::Cut),
                    winit::keyboard::KeyCode::Find => PhysicalKey::Code(KeyCode::Find),
                    winit::keyboard::KeyCode::Open => PhysicalKey::Code(KeyCode::Open),
                    winit::keyboard::KeyCode::Paste => PhysicalKey::Code(KeyCode::Paste),
                    winit::keyboard::KeyCode::Props => PhysicalKey::Code(KeyCode::Props),
                    winit::keyboard::KeyCode::Select => PhysicalKey::Code(KeyCode::Select),
                    winit::keyboard::KeyCode::Undo => PhysicalKey::Code(KeyCode::Undo),
                    winit::keyboard::KeyCode::Hiragana => PhysicalKey::Code(KeyCode::Hiragana),
                    winit::keyboard::KeyCode::Katakana => PhysicalKey::Code(KeyCode::Katakana),
                    winit::keyboard::KeyCode::F1 => PhysicalKey::Code(KeyCode::F1),
                    winit::keyboard::KeyCode::F2 => PhysicalKey::Code(KeyCode::F2),
                    winit::keyboard::KeyCode::F3 => PhysicalKey::Code(KeyCode::F3),
                    winit::keyboard::KeyCode::F4 => PhysicalKey::Code(KeyCode::F4),
                    winit::keyboard::KeyCode::F5 => PhysicalKey::Code(KeyCode::F5),
                    winit::keyboard::KeyCode::F6 => PhysicalKey::Code(KeyCode::F6),
                    winit::keyboard::KeyCode::F7 => PhysicalKey::Code(KeyCode::F7),
                    winit::keyboard::KeyCode::F8 => PhysicalKey::Code(KeyCode::F8),
                    winit::keyboard::KeyCode::F9 => PhysicalKey::Code(KeyCode::F9),
                    winit::keyboard::KeyCode::F10 => PhysicalKey::Code(KeyCode::F10),
                    winit::keyboard::KeyCode::F11 => PhysicalKey::Code(KeyCode::F11),
                    winit::keyboard::KeyCode::F12 => PhysicalKey::Code(KeyCode::F12),
                    winit::keyboard::KeyCode::F13 => PhysicalKey::Code(KeyCode::F13),
                    winit::keyboard::KeyCode::F14 => PhysicalKey::Code(KeyCode::F14),
                    winit::keyboard::KeyCode::F15 => PhysicalKey::Code(KeyCode::F15),
                    winit::keyboard::KeyCode::F16 => PhysicalKey::Code(KeyCode::F16),
                    winit::keyboard::KeyCode::F17 => PhysicalKey::Code(KeyCode::F17),
                    winit::keyboard::KeyCode::F18 => PhysicalKey::Code(KeyCode::F18),
                    winit::keyboard::KeyCode::F19 => PhysicalKey::Code(KeyCode::F19),
                    winit::keyboard::KeyCode::F20 => PhysicalKey::Code(KeyCode::F20),
                    winit::keyboard::KeyCode::F21 => PhysicalKey::Code(KeyCode::F21),
                    winit::keyboard::KeyCode::F22 => PhysicalKey::Code(KeyCode::F22),
                    winit::keyboard::KeyCode::F23 => PhysicalKey::Code(KeyCode::F23),
                    winit::keyboard::KeyCode::F24 => PhysicalKey::Code(KeyCode::F24),
                    winit::keyboard::KeyCode::F25 => PhysicalKey::Code(KeyCode::F25),
                    winit::keyboard::KeyCode::F26 => PhysicalKey::Code(KeyCode::F26),
                    winit::keyboard::KeyCode::F27 => PhysicalKey::Code(KeyCode::F27),
                    winit::keyboard::KeyCode::F28 => PhysicalKey::Code(KeyCode::F28),
                    winit::keyboard::KeyCode::F29 => PhysicalKey::Code(KeyCode::F29),
                    winit::keyboard::KeyCode::F30 => PhysicalKey::Code(KeyCode::F30),
                    winit::keyboard::KeyCode::F31 => PhysicalKey::Code(KeyCode::F31),
                    winit::keyboard::KeyCode::F32 => PhysicalKey::Code(KeyCode::F32),
                    winit::keyboard::KeyCode::F33 => PhysicalKey::Code(KeyCode::F33),
                    winit::keyboard::KeyCode::F34 => PhysicalKey::Code(KeyCode::F34),
                    winit::keyboard::KeyCode::F35 => PhysicalKey::Code(KeyCode::F35),
                    _ => PhysicalKey::Unidentified(0),
                },
                winit::keyboard::PhysicalKey::Unidentified(native_key_code) => {
                    match native_key_code {
                        winit::keyboard::NativeKeyCode::Unidentified => {
                            PhysicalKey::Unidentified(0)
                        }
                        winit::keyboard::NativeKeyCode::Android(c) => PhysicalKey::Unidentified(c),
                        winit::keyboard::NativeKeyCode::MacOS(c) => {
                            PhysicalKey::Unidentified(c as u32)
                        }
                        winit::keyboard::NativeKeyCode::Windows(c) => {
                            PhysicalKey::Unidentified(c as u32)
                        }
                        winit::keyboard::NativeKeyCode::Xkb(c) => PhysicalKey::Unidentified(c),
                    }
                }
            },
            state: match raw_key_event.state {
                winit::event::ElementState::Pressed => ElementState::Pressed,
                winit::event::ElementState::Released => ElementState::Released,
            },
        },
    };
}
