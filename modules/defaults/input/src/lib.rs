use interstice_sdk::{key_code::KeyCode, *};

interstice_module!(visibility: Private, authorities: [Input]);

/// State of each physical key (`code` = `KeyCode` discriminant), plus what the
/// keyboard layout made of its latest transition:
/// - `key`: W3C `KeyboardEvent.key` value (`"a"`, `"é"`, `"Enter"`, `"Shift"`),
///   empty when unknown — consumers then fall back to the physical code;
/// - `text`: the text the latest press typed (empty for releases and keys that
///   type nothing).
#[table(public, ephemeral)]
pub struct KeyState {
    #[primary_key]
    code: u32,
    pressed: bool,
    key: String,
    text: String,
}

/// Keyboard layout used to interpret keys (singleton, id 0):
/// - `"system"` (default): trust what the host resolved with the OS layout,
///   falling back to `"us"` when the host reports nothing;
/// - `"us"` / `"fr"`: built-in QWERTY / AZERTY tables, ignoring the host.
/// Persisted, so a node keeps its setting; change it with
/// `set_keyboard_layout`.
#[table(public, stateful)]
pub struct KeyboardLayout {
    #[primary_key]
    id: u32,
    pub name: String,
}

pub const LAYOUTS: &[&str] = &["system", "us", "fr"];

#[table(public, ephemeral)]
pub struct MouseState {
    #[primary_key]
    id: u32,
    position: (f32, f32),
    wheel_delta: (f32, f32),
}

/// Per-button mouse state. `button_id` is semantic (Left=0, Right=1, Middle=2;
/// see the runtime mapping in interstice-core app.rs). Rows for 0/1/2 are seeded
/// on load so consumers can always `get()` them.
#[table(public, ephemeral)]
pub struct MouseButton {
    #[primary_key]
    button_id: u32,
    pressed: bool,
}

/// Holds the character typed in the most recent key-press event.
/// Special values: character = "\x08" means Backspace, "" means no printable input.
#[table(public, ephemeral)]
pub struct TextInputBuffer {
    #[primary_key]
    id: u32,
    pub character: String,
}

#[reducer(on = "load")]
fn on_load<Caps>(ctx: ReducerContext<Caps>) -> Result<(), String>
where
    Caps: CanInsert<KeyState>
        + CanInsert<MouseState>
        + CanInsert<MouseButton>
        + CanInsert<TextInputBuffer>
        + CanRead<KeyboardLayout>
        + CanInsert<KeyboardLayout>,
{
    let res = ctx.current.tables.mousestate().insert(MouseState {
        id: 0,
        position: (0.0, 0.0),
        wheel_delta: (0.0, 0.0),
    });
    if let Err(err) = res {
        ctx.log(&format!("Failed to initialize mouse state: {}", err));
    }

    for button_id in 0..3 {
        let _ = ctx.current.tables.mousebutton().insert(MouseButton {
            button_id,
            pressed: false,
        });
    }

    let _ = ctx
        .current
        .tables
        .textinputbuffer()
        .insert(TextInputBuffer {
            id: 0,
            character: String::new(),
        });

    if ctx.current.tables.keyboardlayout().get(0)?.is_none() {
        let _ = ctx.current.tables.keyboardlayout().insert(KeyboardLayout {
            id: 0,
            name: "system".into(),
        });
    }

    for code in KeyCode::iter() {
        let res = ctx.current.tables.keystate().insert(KeyState {
            code: code.clone() as u32,
            pressed: false,
            key: String::new(),
            text: String::new(),
        });
        if let Err(err) = res {
            ctx.log(&format!(
                "Failed to initialize key state for code {}: {}",
                code as u32, err
            ));
        }
    }
    Ok(())
}

/// Select the keyboard layout (one of `LAYOUTS`).
#[reducer]
fn set_keyboard_layout<Caps>(ctx: ReducerContext<Caps>, name: String) -> Result<(), String>
where
    Caps: CanRead<KeyboardLayout> + CanInsert<KeyboardLayout> + CanUpdate<KeyboardLayout>,
{
    if !LAYOUTS.contains(&name.as_str()) {
        ctx.log(&format!(
            "Unknown keyboard layout {name:?} (expected one of {LAYOUTS:?})"
        ));
        return Ok(());
    }
    let row = KeyboardLayout { id: 0, name };
    let t = ctx.current.tables.keyboardlayout();
    if t.get(0)?.is_some() {
        let _ = t.update(row);
    } else {
        let _ = t.insert(row);
    }
    Ok(())
}

#[reducer(on = "input")]
fn on_input<Caps>(ctx: ReducerContext<Caps>, event: InputEvent) -> Result<(), String>
where
    Caps: CanRead<MouseState>
        + CanUpdate<MouseState>
        + CanRead<MouseButton>
        + CanInsert<MouseButton>
        + CanUpdate<MouseButton>
        + CanRead<KeyState>
        + CanInsert<KeyState>
        + CanUpdate<KeyState>
        + CanRead<TextInputBuffer>
        + CanUpdate<TextInputBuffer>
        + CanRead<KeyboardLayout>,
{
    match event {
        InputEvent::Added { .. } => {}
        InputEvent::Removed { .. } => {}
        InputEvent::MouseMotion { delta, .. } => {
            let mouse_state = ctx
                .current
                .tables
                .mousestate()
                .get(0)?
                .ok_or("mouse state row missing")?;
            let new_position = (
                mouse_state.position.0 + delta.0 as f32,
                mouse_state.position.1 + delta.1 as f32,
            );
            let _ = ctx.current.tables.mousestate().update(MouseState {
                id: 0,
                position: new_position,
                wheel_delta: mouse_state.wheel_delta,
            });
        }
        InputEvent::MouseWheel { delta, .. } => {
            let mouse_state = ctx
                .current
                .tables
                .mousestate()
                .get(0)?
                .ok_or("mouse state row missing")?;
            let new_wheel_delta = (
                mouse_state.wheel_delta.0 + delta.0 as f32,
                mouse_state.wheel_delta.1 + delta.1 as f32,
            );
            let _ = ctx.current.tables.mousestate().update(MouseState {
                id: 0,
                position: mouse_state.position,
                wheel_delta: new_wheel_delta,
            });
        }
        InputEvent::Motion { .. } => {}
        InputEvent::Button {
            button_id, state, ..
        } => {
            let pressed = matches!(state, ElementState::Pressed);
            let row = MouseButton { button_id, pressed };
            if ctx.current.tables.mousebutton().get(button_id)?.is_some() {
                let _ = ctx.current.tables.mousebutton().update(row);
            } else {
                let _ = ctx.current.tables.mousebutton().insert(row);
            }
        }
        InputEvent::Key {
            physical_key,
            state,
            logical_key,
            text,
            ..
        } => {
            let pressed = matches!(state, ElementState::Pressed);

            let (code, key_code_opt) = match physical_key {
                PhysicalKey::Code(kc) => (kc.clone() as u32, Some(kc)),
                PhysicalKey::Unidentified(c) => (c, None),
            };

            let layout = ctx
                .current
                .tables
                .keyboardlayout()
                .get(0)?
                .map(|l| l.name)
                .unwrap_or_default();
            let host_resolved = !logical_key.is_empty() || !text.is_empty();
            let (key, text) = if layout == "system" && host_resolved {
                (logical_key, printable(text))
            } else {
                // Built-in table: printable keys only; named keys keep "".
                let shift = is_shift_pressed(&ctx)?;
                let ch = key_code_opt.as_ref().and_then(|kc| {
                    if layout == "fr" { fr_char(kc, shift) } else { us_char(kc, shift) }
                });
                let ch = ch.map(String::from).unwrap_or_default();
                (ch.clone(), if pressed { ch } else { String::new() })
            };

            let current = ctx.current.tables.keystate().get(code)?;
            let was_pressed = current.as_ref().map(|ks| ks.pressed).unwrap_or(false);

            let new_ks = KeyState {
                code,
                pressed,
                key,
                text: if pressed { text.clone() } else { String::new() },
            };
            match current {
                None => {
                    let _ = ctx.current.tables.keystate().insert(new_ks);
                }
                Some(_) => {
                    let _ = ctx.current.tables.keystate().update(new_ks);
                }
            }
            if pressed && !was_pressed {
                let typed = if matches!(key_code_opt, Some(KeyCode::Backspace)) {
                    Some("\x08".to_string())
                } else if text.is_empty() {
                    None
                } else {
                    Some(text)
                };
                if let Some(ch) = typed {
                    if let Some(mut buf) = ctx.current.tables.textinputbuffer().get(0)? {
                        buf.character = ch;
                        let _ = ctx.current.tables.textinputbuffer().update(buf);
                    }
                }
            }
        }
    }
    Ok(())
}

/// Typed text without control characters (hosts report e.g. "\r" for
/// Enter, "\x08" for Backspace, "\x1b" for Escape).
fn printable(text: String) -> String {
    if text.chars().any(char::is_control) {
        text.chars().filter(|c| !c.is_control()).collect()
    } else {
        text
    }
}

fn is_shift_pressed<Caps>(ctx: &ReducerContext<Caps>) -> Result<bool, String>
where
    Caps: CanRead<KeyState>,
{
    let shift_l = KeyCode::ShiftLeft as u32;
    let shift_r = KeyCode::ShiftRight as u32;
    Ok(ctx
        .current
        .tables
        .keystate()
        .scan()?
        .iter()
        .any(|k| (k.code == shift_l || k.code == shift_r) && k.pressed))
}

/// Character a physical key types on a US QWERTY layout (None if none).
fn us_char(kc: &KeyCode, shift: bool) -> Option<char> {
    let ch = match kc {
        KeyCode::KeyA => {
            if shift {
                'A'
            } else {
                'a'
            }
        }
        KeyCode::KeyB => {
            if shift {
                'B'
            } else {
                'b'
            }
        }
        KeyCode::KeyC => {
            if shift {
                'C'
            } else {
                'c'
            }
        }
        KeyCode::KeyD => {
            if shift {
                'D'
            } else {
                'd'
            }
        }
        KeyCode::KeyE => {
            if shift {
                'E'
            } else {
                'e'
            }
        }
        KeyCode::KeyF => {
            if shift {
                'F'
            } else {
                'f'
            }
        }
        KeyCode::KeyG => {
            if shift {
                'G'
            } else {
                'g'
            }
        }
        KeyCode::KeyH => {
            if shift {
                'H'
            } else {
                'h'
            }
        }
        KeyCode::KeyI => {
            if shift {
                'I'
            } else {
                'i'
            }
        }
        KeyCode::KeyJ => {
            if shift {
                'J'
            } else {
                'j'
            }
        }
        KeyCode::KeyK => {
            if shift {
                'K'
            } else {
                'k'
            }
        }
        KeyCode::KeyL => {
            if shift {
                'L'
            } else {
                'l'
            }
        }
        KeyCode::KeyM => {
            if shift {
                'M'
            } else {
                'm'
            }
        }
        KeyCode::KeyN => {
            if shift {
                'N'
            } else {
                'n'
            }
        }
        KeyCode::KeyO => {
            if shift {
                'O'
            } else {
                'o'
            }
        }
        KeyCode::KeyP => {
            if shift {
                'P'
            } else {
                'p'
            }
        }
        KeyCode::KeyQ => {
            if shift {
                'Q'
            } else {
                'q'
            }
        }
        KeyCode::KeyR => {
            if shift {
                'R'
            } else {
                'r'
            }
        }
        KeyCode::KeyS => {
            if shift {
                'S'
            } else {
                's'
            }
        }
        KeyCode::KeyT => {
            if shift {
                'T'
            } else {
                't'
            }
        }
        KeyCode::KeyU => {
            if shift {
                'U'
            } else {
                'u'
            }
        }
        KeyCode::KeyV => {
            if shift {
                'V'
            } else {
                'v'
            }
        }
        KeyCode::KeyW => {
            if shift {
                'W'
            } else {
                'w'
            }
        }
        KeyCode::KeyX => {
            if shift {
                'X'
            } else {
                'x'
            }
        }
        KeyCode::KeyY => {
            if shift {
                'Y'
            } else {
                'y'
            }
        }
        KeyCode::KeyZ => {
            if shift {
                'Z'
            } else {
                'z'
            }
        }
        KeyCode::Digit0 => {
            if shift {
                ')'
            } else {
                '0'
            }
        }
        KeyCode::Digit1 => {
            if shift {
                '!'
            } else {
                '1'
            }
        }
        KeyCode::Digit2 => {
            if shift {
                '@'
            } else {
                '2'
            }
        }
        KeyCode::Digit3 => {
            if shift {
                '#'
            } else {
                '3'
            }
        }
        KeyCode::Digit4 => {
            if shift {
                '$'
            } else {
                '4'
            }
        }
        KeyCode::Digit5 => {
            if shift {
                '%'
            } else {
                '5'
            }
        }
        KeyCode::Digit6 => {
            if shift {
                '^'
            } else {
                '6'
            }
        }
        KeyCode::Digit7 => {
            if shift {
                '&'
            } else {
                '7'
            }
        }
        KeyCode::Digit8 => {
            if shift {
                '*'
            } else {
                '8'
            }
        }
        KeyCode::Digit9 => {
            if shift {
                '('
            } else {
                '9'
            }
        }
        KeyCode::Space => ' ',
        KeyCode::Minus => {
            if shift {
                '_'
            } else {
                '-'
            }
        }
        KeyCode::Equal => {
            if shift {
                '+'
            } else {
                '='
            }
        }
        KeyCode::Period => {
            if shift {
                '>'
            } else {
                '.'
            }
        }
        KeyCode::Comma => {
            if shift {
                '<'
            } else {
                ','
            }
        }
        KeyCode::Slash => {
            if shift {
                '?'
            } else {
                '/'
            }
        }
        KeyCode::Backslash => {
            if shift {
                '|'
            } else {
                '\\'
            }
        }
        KeyCode::Semicolon => {
            if shift {
                ':'
            } else {
                ';'
            }
        }
        KeyCode::Quote => {
            if shift {
                '"'
            } else {
                '\''
            }
        }
        KeyCode::BracketLeft => {
            if shift {
                '{'
            } else {
                '['
            }
        }
        KeyCode::BracketRight => {
            if shift {
                '}'
            } else {
                ']'
            }
        }
        KeyCode::Backquote => {
            if shift {
                '~'
            } else {
                '`'
            }
        }
        _ => return None,
    };
    Some(ch)
}

/// Character a physical key types on a French AZERTY layout (None if none,
/// including the dead `^` key).
fn fr_char(kc: &KeyCode, shift: bool) -> Option<char> {
    let pick = |a: char, b: char| Some(if shift { b } else { a });
    match kc {
        KeyCode::KeyQ => pick('a', 'A'),
        KeyCode::KeyA => pick('q', 'Q'),
        KeyCode::KeyW => pick('z', 'Z'),
        KeyCode::KeyZ => pick('w', 'W'),
        KeyCode::Semicolon => pick('m', 'M'),
        KeyCode::KeyM => pick(',', '?'),
        KeyCode::Comma => pick(';', '.'),
        KeyCode::Period => pick(':', '/'),
        KeyCode::Slash => pick('!', '§'),
        KeyCode::Quote => pick('ù', '%'),
        KeyCode::Backquote => pick('²', '²'),
        KeyCode::BracketRight => pick('$', '£'),
        KeyCode::Backslash => pick('*', 'µ'),
        KeyCode::IntlBackslash => pick('<', '>'),
        KeyCode::Minus => pick(')', '°'),
        KeyCode::Equal => pick('=', '+'),
        KeyCode::Digit1 => pick('&', '1'),
        KeyCode::Digit2 => pick('é', '2'),
        KeyCode::Digit3 => pick('"', '3'),
        KeyCode::Digit4 => pick('\'', '4'),
        KeyCode::Digit5 => pick('(', '5'),
        KeyCode::Digit6 => pick('-', '6'),
        KeyCode::Digit7 => pick('è', '7'),
        KeyCode::Digit8 => pick('_', '8'),
        KeyCode::Digit9 => pick('ç', '9'),
        KeyCode::Digit0 => pick('à', '0'),
        KeyCode::BracketLeft => None,
        kc => us_char(kc, shift),
    }
}
