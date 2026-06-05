use interstice_sdk::{key_code::KeyCode, *};

interstice_module!(visibility: Private, authorities: [Input]);

#[table(public, ephemeral)]
pub struct KeyState {
    #[primary_key]
    code: u32,
    pressed: bool,
    // X11 auto-repeat detection: set true on Released so the very next Pressed
    // for the same key can be identified as a repeat and not emit a character.
    // Consumers that only read `pressed` (e.g. agar-client) see this as an
    // extra trailing field and ignore it safely.
    pending_release: bool,
}

#[table(public, ephemeral)]
pub struct MouseState {
    #[primary_key]
    id: u32,
    position: (f32, f32),
    wheel_delta: (f32, f32),
}

/// Holds the character typed in the most recent key-press event.
/// `generation` increments on every fresh key press so consumers can detect
/// new events even when the same character is typed twice in succession.
/// Special values: character = "\x08" means Backspace, "" means no printable input.
#[table(public, ephemeral)]
pub struct TextInputBuffer {
    #[primary_key]
    id: u32,
    pub character: String,
    pub generation: u64,
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
        .insert(TextInputBuffer { id: 0, character: String::new(), generation: 0 });

    for code in KeyCode::iter() {
        let res = ctx.current.tables.keystate().insert(KeyState {
            code: code.clone() as u32,
            pressed: false,
            pending_release: false,
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
        InputEvent::Added { .. } => {
            flush_pending_releases(&ctx);
        }
        InputEvent::Removed { .. } => {
            flush_pending_releases(&ctx);
        }
        InputEvent::MouseMotion { delta, .. } => {
            flush_pending_releases(&ctx);
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
            flush_pending_releases(&ctx);
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
        InputEvent::Motion { .. } => {
            flush_pending_releases(&ctx);
        }
        InputEvent::Button { .. } => {
            flush_pending_releases(&ctx);
        }
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

            // Flush pending_release marks for ALL OTHER keys — their release was
            // genuine since no immediate same-key Press arrived.
            for mut ks in ctx.current.tables.keystate().scan() {
                if ks.pending_release && ks.code != code {
                    ks.pending_release = false;
                    let _ = ctx.current.tables.keystate().update(ks);
                }
            }

            let current = ctx.current.tables.keystate().get(code);
            let is_pending_release = current.as_ref().map(|ks| ks.pending_release).unwrap_or(false);
            let was_pressed = current.as_ref().map(|ks| ks.pressed).unwrap_or(false);

            if pressed {
                // Mark as pressed, clear the pending_release flag.
                let new_ks = KeyState { code, pressed: true, pending_release: false };
                match current {
                    None => { let _ = ctx.current.tables.keystate().insert(new_ks); }
                    Some(_) => { let _ = ctx.current.tables.keystate().update(new_ks); }
                }

                // Only generate text on a FRESH key-down:
                //   • is_pending_release=true  → X11 auto-repeat (Release+Press pair), skip
                //   • was_pressed=true          → winit key-repeat (consecutive Pressed), skip
                if !is_pending_release && !was_pressed {
                    if let Some(kc) = key_code_opt {
                        let shift = is_shift_pressed(&ctx);
                        if let Some(ch) = key_to_char(&kc, shift) {
                            if let Some(mut buf) = ctx.current.tables.textinputbuffer().get(0) {
                                buf.character = ch;
                                buf.generation = buf.generation.wrapping_add(1);
                                let _ = ctx.current.tables.textinputbuffer().update(buf);
                            }
                        }
                    }
                }
            } else {
                // Released: mark pending_release so an immediate same-key Press is
                // recognised as X11 auto-repeat rather than a fresh keystroke.
                let new_ks = KeyState { code, pressed: false, pending_release: true };
                match current {
                    None => { let _ = ctx.current.tables.keystate().insert(new_ks); }
                    Some(_) => { let _ = ctx.current.tables.keystate().update(new_ks); }
                }
            }
        }
    }
}

/// Clear all pending_release flags. Called on non-key events so that keys released
/// without an immediate X11 auto-repeat Press are fully committed as released.
fn flush_pending_releases<Caps>(ctx: &ReducerContext<Caps>)
where
    Caps: CanRead<KeyState> + CanUpdate<KeyState>,
{
    for mut ks in ctx.current.tables.keystate().scan() {
        if ks.pending_release {
            ks.pending_release = false;
            let _ = ctx.current.tables.keystate().update(ks);
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
        KeyCode::KeyA => if shift { 'A' } else { 'a' },
        KeyCode::KeyB => if shift { 'B' } else { 'b' },
        KeyCode::KeyC => if shift { 'C' } else { 'c' },
        KeyCode::KeyD => if shift { 'D' } else { 'd' },
        KeyCode::KeyE => if shift { 'E' } else { 'e' },
        KeyCode::KeyF => if shift { 'F' } else { 'f' },
        KeyCode::KeyG => if shift { 'G' } else { 'g' },
        KeyCode::KeyH => if shift { 'H' } else { 'h' },
        KeyCode::KeyI => if shift { 'I' } else { 'i' },
        KeyCode::KeyJ => if shift { 'J' } else { 'j' },
        KeyCode::KeyK => if shift { 'K' } else { 'k' },
        KeyCode::KeyL => if shift { 'L' } else { 'l' },
        KeyCode::KeyM => if shift { 'M' } else { 'm' },
        KeyCode::KeyN => if shift { 'N' } else { 'n' },
        KeyCode::KeyO => if shift { 'O' } else { 'o' },
        KeyCode::KeyP => if shift { 'P' } else { 'p' },
        KeyCode::KeyQ => if shift { 'Q' } else { 'q' },
        KeyCode::KeyR => if shift { 'R' } else { 'r' },
        KeyCode::KeyS => if shift { 'S' } else { 's' },
        KeyCode::KeyT => if shift { 'T' } else { 't' },
        KeyCode::KeyU => if shift { 'U' } else { 'u' },
        KeyCode::KeyV => if shift { 'V' } else { 'v' },
        KeyCode::KeyW => if shift { 'W' } else { 'w' },
        KeyCode::KeyX => if shift { 'X' } else { 'x' },
        KeyCode::KeyY => if shift { 'Y' } else { 'y' },
        KeyCode::KeyZ => if shift { 'Z' } else { 'z' },
        KeyCode::Digit0 => if shift { ')' } else { '0' },
        KeyCode::Digit1 => if shift { '!' } else { '1' },
        KeyCode::Digit2 => if shift { '@' } else { '2' },
        KeyCode::Digit3 => if shift { '#' } else { '3' },
        KeyCode::Digit4 => if shift { '$' } else { '4' },
        KeyCode::Digit5 => if shift { '%' } else { '5' },
        KeyCode::Digit6 => if shift { '^' } else { '6' },
        KeyCode::Digit7 => if shift { '&' } else { '7' },
        KeyCode::Digit8 => if shift { '*' } else { '8' },
        KeyCode::Digit9 => if shift { '(' } else { '9' },
        KeyCode::Space => ' ',
        KeyCode::Minus => if shift { '_' } else { '-' },
        KeyCode::Equal => if shift { '+' } else { '=' },
        KeyCode::Period => if shift { '>' } else { '.' },
        KeyCode::Comma => if shift { '<' } else { ',' },
        KeyCode::Slash => if shift { '?' } else { '/' },
        KeyCode::Backslash => if shift { '|' } else { '\\' },
        KeyCode::Semicolon => if shift { ':' } else { ';' },
        KeyCode::Quote => if shift { '"' } else { '\'' },
        KeyCode::BracketLeft => if shift { '{' } else { '[' },
        KeyCode::BracketRight => if shift { '}' } else { ']' },
        KeyCode::Backquote => if shift { '~' } else { '`' },
        KeyCode::Backspace => return Some("\x08".to_string()),
        _ => return None,
    };
    Some(ch.to_string())
}
