//! Keyboard translation: the input module reports each key's physical code
//! (the `KeyCode` discriminant as a `u32`) and, when known, the logical key
//! and typed text its keyboard layout produced. Blitz wants DOM-style key
//! events: the physical code becomes `code` (by name), the logical key
//! `key`, and the typed text `text`. Without a logical key (host or headless
//! script couldn't resolve one) we fall back to a US layout.

use blitz_traits::events::{BlitzKeyEvent, KeyState};
use keyboard_types::{Code, Key, Location, Modifiers, NamedKey};
use std::str::FromStr;

/// `KeyCode` variant names in declaration order (index = discriminant).
const KEYCODE_NAMES: &[&str] = &["Backquote","Backslash","BracketLeft","BracketRight","Comma","Digit0","Digit1","Digit2","Digit3","Digit4","Digit5","Digit6","Digit7","Digit8","Digit9","Equal","IntlBackslash","IntlRo","IntlYen","KeyA","KeyB","KeyC","KeyD","KeyE","KeyF","KeyG","KeyH","KeyI","KeyJ","KeyK","KeyL","KeyM","KeyN","KeyO","KeyP","KeyQ","KeyR","KeyS","KeyT","KeyU","KeyV","KeyW","KeyX","KeyY","KeyZ","Minus","Period","Quote","Semicolon","Slash","AltLeft","AltRight","Backspace","CapsLock","ContextMenu","ControlLeft","ControlRight","Enter","SuperLeft","SuperRight","ShiftLeft","ShiftRight","Space","Tab","Convert","KanaMode","Lang1","Lang2","Lang3","Lang4","Lang5","NonConvert","Delete","End","Help","Home","Insert","PageDown","PageUp","ArrowDown","ArrowLeft","ArrowRight","ArrowUp","NumLock","Numpad0","Numpad1","Numpad2","Numpad3","Numpad4","Numpad5","Numpad6","Numpad7","Numpad8","Numpad9","NumpadAdd","NumpadBackspace","NumpadClear","NumpadClearEntry","NumpadComma","NumpadDecimal","NumpadDivide","NumpadEnter","NumpadEqual","NumpadHash","NumpadMemoryAdd","NumpadMemoryClear","NumpadMemoryRecall","NumpadMemoryStore","NumpadMemorySubtract","NumpadMultiply","NumpadParenLeft","NumpadParenRight","NumpadStar","NumpadSubtract","Escape","Fn","FnLock","PrintScreen","ScrollLock","Pause","BrowserBack","BrowserFavorites","BrowserForward","BrowserHome","BrowserRefresh","BrowserSearch","BrowserStop","Eject","LaunchApp1","LaunchApp2","LaunchMail","MediaPlayPause","MediaSelect","MediaStop","MediaTrackNext","MediaTrackPrevious","Power","Sleep","AudioVolumeDown","AudioVolumeMute","AudioVolumeUp","WakeUp","Meta","Hyper","Turbo","Abort","Resume","Suspend","Again","Copy","Cut","Find","Open","Paste","Props","Select","Undo","Hiragana","Katakana","F1","F2","F3","F4","F5","F6","F7","F8","F9","F10","F11","F12","F13","F14","F15","F16","F17","F18","F19","F20","F21","F22","F23","F24","F25","F26","F27","F28","F29","F30","F31","F32","F33","F34","F35",];

pub fn code_name(code: u32) -> Option<&'static str> {
    KEYCODE_NAMES.get(code as usize).copied()
}

/// Inverse of `code_name`; also accepts `Control`/`Shift`/`Alt`/`Meta`.
pub fn code_of(name: &str) -> Option<u32> {
    let name = match name {
        "Control" | "Ctrl" => "ControlLeft",
        "Shift" => "ShiftLeft",
        "Alt" => "AltLeft",
        "Meta" | "Super" => "SuperLeft",
        n => n,
    };
    KEYCODE_NAMES.iter().position(|n| *n == name).map(|i| i as u32)
}

/// Physical key (and whether Shift is needed) typing `c` on a US layout
/// (headless `type`; other characters are sent with an unknown code).
pub fn code_for_char(c: char) -> Option<(&'static str, bool)> {
    const LETTERS: &[&str] = &["KeyA","KeyB","KeyC","KeyD","KeyE","KeyF","KeyG","KeyH","KeyI","KeyJ","KeyK","KeyL","KeyM","KeyN","KeyO","KeyP","KeyQ","KeyR","KeyS","KeyT","KeyU","KeyV","KeyW","KeyX","KeyY","KeyZ"];
    const DIGITS: &[&str] = &["Digit0","Digit1","Digit2","Digit3","Digit4","Digit5","Digit6","Digit7","Digit8","Digit9"];
    Some(match c {
        'a'..='z' => (LETTERS[c as usize - 'a' as usize], false),
        'A'..='Z' => (LETTERS[c as usize - 'A' as usize], true),
        '0'..='9' => (DIGITS[c as usize - '0' as usize], false),
        ' ' => ("Space", false),
        '.' => ("Period", false),
        ',' => ("Comma", false),
        '/' => ("Slash", false),
        '-' => ("Minus", false),
        '=' => ("Equal", false),
        ';' => ("Semicolon", false),
        ':' => ("Semicolon", true),
        '\'' => ("Quote", false),
        '?' => ("Slash", true),
        '_' => ("Minus", true),
        _ => return None,
    })
}

/// Current modifier state, tracked from key presses.
#[derive(Default, Clone, Copy)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

impl Mods {
    pub fn update(&mut self, name: &str, pressed: bool) {
        match name {
            "ShiftLeft" | "ShiftRight" => self.shift = pressed,
            "ControlLeft" | "ControlRight" => self.ctrl = pressed,
            "AltLeft" | "AltRight" => self.alt = pressed,
            "SuperLeft" | "SuperRight" | "Meta" => self.meta = pressed,
            _ => {}
        }
    }
    pub fn bits(self) -> u8 {
        self.shift as u8 | (self.ctrl as u8) << 1 | (self.alt as u8) << 2 | (self.meta as u8) << 3
    }
    pub fn from_bits(b: u8) -> Self {
        Mods { shift: b & 1 != 0, ctrl: b & 2 != 0, alt: b & 4 != 0, meta: b & 8 != 0 }
    }
    pub fn to_modifiers(self) -> Modifiers {
        let mut m = Modifiers::empty();
        if self.shift {
            m |= Modifiers::SHIFT;
        }
        if self.ctrl {
            m |= Modifiers::CONTROL;
        }
        if self.alt {
            m |= Modifiers::ALT;
        }
        if self.meta {
            m |= Modifiers::META;
        }
        m
    }
}

/// Printable character for a physical key on a US layout.
fn us_char(name: &str, shift: bool) -> Option<char> {
    if let Some(l) = name.strip_prefix("Key") {
        let c = l.chars().next()?;
        return Some(if shift { c } else { c.to_ascii_lowercase() });
    }
    if let Some(d) = name.strip_prefix("Digit") {
        let c = d.chars().next()?;
        if !shift {
            return Some(c);
        }
        return Some(match c {
            '1' => '!',
            '2' => '@',
            '3' => '#',
            '4' => '$',
            '5' => '%',
            '6' => '^',
            '7' => '&',
            '8' => '*',
            '9' => '(',
            _ => ')',
        });
    }
    if let Some(d) = name.strip_prefix("Numpad") {
        if d.len() == 1 {
            return d.chars().next();
        }
    }
    let (a, b) = match name {
        "Space" => (' ', ' '),
        "Minus" => ('-', '_'),
        "Equal" => ('=', '+'),
        "BracketLeft" => ('[', '{'),
        "BracketRight" => (']', '}'),
        "Backslash" => ('\\', '|'),
        "Semicolon" => (';', ':'),
        "Quote" => ('\'', '"'),
        "Comma" => (',', '<'),
        "Period" => ('.', '>'),
        "Slash" => ('/', '?'),
        "Backquote" => ('`', '~'),
        "NumpadAdd" => ('+', '+'),
        "NumpadSubtract" => ('-', '-'),
        "NumpadMultiply" => ('*', '*'),
        "NumpadDivide" => ('/', '/'),
        "NumpadDecimal" => ('.', '.'),
        _ => return None,
    };
    Some(if shift { b } else { a })
}

fn named(name: &str) -> Option<NamedKey> {
    Some(match name {
        "Enter" | "NumpadEnter" => NamedKey::Enter,
        "Backspace" | "NumpadBackspace" => NamedKey::Backspace,
        "Tab" => NamedKey::Tab,
        "Escape" => NamedKey::Escape,
        "Delete" => NamedKey::Delete,
        "Insert" => NamedKey::Insert,
        "Home" => NamedKey::Home,
        "End" => NamedKey::End,
        "PageUp" => NamedKey::PageUp,
        "PageDown" => NamedKey::PageDown,
        "ArrowUp" => NamedKey::ArrowUp,
        "ArrowDown" => NamedKey::ArrowDown,
        "ArrowLeft" => NamedKey::ArrowLeft,
        "ArrowRight" => NamedKey::ArrowRight,
        "ShiftLeft" | "ShiftRight" => NamedKey::Shift,
        "ControlLeft" | "ControlRight" => NamedKey::Control,
        "AltLeft" | "AltRight" => NamedKey::Alt,
        "SuperLeft" | "SuperRight" | "Meta" => NamedKey::Meta,
        "CapsLock" => NamedKey::CapsLock,
        "ContextMenu" => NamedKey::ContextMenu,
        n if n.starts_with('F') && n[1..].parse::<u8>().is_ok() => {
            NamedKey::from_str(n).ok()?
        }
        _ => return None,
    })
}

/// Name used to match keyboard shortcuts: the logical letter when the
/// layout produced one (Ctrl+W is the key labelled W, wherever it sits),
/// otherwise the physical key (digits, F-keys, arrows…).
pub fn shortcut_name(physical: &'static str, key: &str) -> std::borrow::Cow<'static, str> {
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphabetic() => format!("Key{}", c.to_ascii_uppercase()).into(),
        _ => physical.into(),
    }
}

/// Build the Blitz key event for a key transition. `name` is the physical
/// key (`code_name`, "" if unknown); `key` / `text` the layout's logical key
/// and typed text ("" = unknown → US layout).
pub fn key_event(name: &str, key: &str, text: &str, pressed: bool, mods: Mods) -> Option<BlitzKeyEvent> {
    let dom_code = Code::from_str(name).unwrap_or(Code::Unidentified);
    let (key, text) = if let Some(n) = named(name) {
        let text = match n {
            NamedKey::Enter => Some("\r".into()),
            _ => None,
        };
        (Key::Named(n), text)
    } else if !key.is_empty() {
        let key = Key::from_str(key).unwrap_or(Key::Named(NamedKey::Unidentified));
        let text = match &key {
            Key::Character(c) if pressed => Some(if text.is_empty() { c.as_str() } else { text }.into()),
            _ => None,
        };
        (key, text)
    } else if let Some(c) = us_char(name, mods.shift) {
        (Key::Character(c.to_string()), Some(c.to_string().into()))
    } else if name.is_empty() {
        return None;
    } else {
        (Key::Named(NamedKey::Unidentified), None)
    };
    let location = if name.ends_with("Left") && named(name).is_some() && !name.starts_with("Arrow") {
        Location::Left
    } else if name.ends_with("Right") && named(name).is_some() && !name.starts_with("Arrow") {
        Location::Right
    } else if name.starts_with("Numpad") {
        Location::Numpad
    } else {
        Location::Standard
    };
    Some(BlitzKeyEvent {
        key,
        code: dom_code,
        modifiers: mods.to_modifiers(),
        location,
        is_auto_repeating: false,
        is_composing: false,
        state: if pressed { KeyState::Pressed } else { KeyState::Released },
        // Ctrl/Meta chords are shortcuts, not text.
        text: if mods.ctrl || mods.meta { None } else { text },
    })
}
