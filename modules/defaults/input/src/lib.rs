use interstice_sdk::{key_code::KeyCode, *};

interstice_module!(visibility: Private, authorities: [Input]);

#[table(public, ephemeral)]
pub struct KeyState {
    #[primary_key]
    code: u32,
    pressed: bool,
}

#[table(public, ephemeral)]
pub struct MouseState {
    #[primary_key]
    id: u32,
    position: (f32, f32),
    wheel_delta: (f32, f32),
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
fn on_load<Caps>(ctx: ReducerContext<Caps>)
where
    Caps: CanInsert<KeyState> + CanInsert<MouseState> + CanInsert<TextInputBuffer>,
{
    let res = ctx.current.tables.mousestate().insert(MouseState {
        id: 0,
        position: (0.0, 0.0),
        wheel_delta: (0.0, 0.0),
    });
    if let Err(err) = res {
        ctx.log(&format!("Failed to initialize mouse state: {}", err));
    }

    let _ = ctx
        .current
        .tables
        .textinputbuffer()
        .insert(TextInputBuffer {
            id: 0,
            character: String::new(),
        });

    for code in KeyCode::iter() {
        let res = ctx.current.tables.keystate().insert(KeyState {
            code: code.clone() as u32,
            pressed: false,
        });
        if let Err(err) = res {
            ctx.log(&format!(
                "Failed to initialize key state for code {}: {}",
                code as u32, err
            ));
        }
    }
}

#[reducer(on = "input")]
fn on_input<Caps>(ctx: ReducerContext<Caps>, event: InputEvent)
where
    Caps: CanRead<MouseState>
        + CanUpdate<MouseState>
        + CanRead<KeyState>
        + CanInsert<KeyState>
        + CanUpdate<KeyState>
        + CanRead<TextInputBuffer>
        + CanUpdate<TextInputBuffer>,
{
    match event {
        InputEvent::Added { .. } => {}
        InputEvent::Removed { .. } => {}
        InputEvent::MouseMotion { delta, .. } => {
            let mouse_state = ctx.current.tables.mousestate().get(0).unwrap();
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
            let mouse_state = ctx.current.tables.mousestate().get(0).unwrap();
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
        InputEvent::Button { .. } => {}
        InputEvent::Key {
            physical_key,
            state,
            ..
        } => {
            let pressed = matches!(state, ElementState::Pressed);

            let (code, key_code_opt) = match physical_key {
                PhysicalKey::Code(kc) => (kc.clone() as u32, Some(kc)),
                PhysicalKey::Unidentified(c) => (c, None),
            };

            let current = ctx.current.tables.keystate().get(code);
            let was_pressed = current.as_ref().map(|ks| ks.pressed).unwrap_or(false);

            let new_ks = KeyState { code, pressed };
            match current {
                None => {
                    let _ = ctx.current.tables.keystate().insert(new_ks);
                }
                Some(_) => {
                    let _ = ctx.current.tables.keystate().update(new_ks);
                }
            }
            if pressed && !was_pressed {
                if let Some(kc) = key_code_opt {
                    let shift = is_shift_pressed(&ctx);
                    if let Some(ch) = key_to_char(&kc, shift) {
                        if let Some(mut buf) = ctx.current.tables.textinputbuffer().get(0) {
                            buf.character = ch;
                            let _ = ctx.current.tables.textinputbuffer().update(buf);
                        }
                    }
                }
            }
        }
    }
}

fn is_shift_pressed<Caps>(ctx: &ReducerContext<Caps>) -> bool
where
    Caps: CanRead<KeyState>,
{
    let shift_l = KeyCode::ShiftLeft as u32;
    let shift_r = KeyCode::ShiftRight as u32;
    ctx.current
        .tables
        .keystate()
        .scan()
        .iter()
        .any(|k| (k.code == shift_l || k.code == shift_r) && k.pressed)
}

/// Maps a KeyCode to a text character. Returns None for non-printable keys.
/// Returns "\x08" for Backspace so consumers can handle deletion.
fn key_to_char(kc: &KeyCode, shift: bool) -> Option<String> {
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
        KeyCode::Backspace => return Some("\x08".to_string()),
        _ => return None,
    };
    Some(ch.to_string())
}
