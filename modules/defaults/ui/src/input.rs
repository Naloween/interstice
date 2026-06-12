use crate::bindings::input::*;
use crate::tables::*;
use crate::text::*;
use interstice_sdk::*;

#[reducer(on = "input.textinputbuffer.update")]
pub fn on_key_input<Caps>(
    ctx: ReducerContext<Caps>,
    _previous_buf: TextInputBuffer,
    new_buf: TextInputBuffer,
) where
    Caps: CanRead<InputFocus>
        + CanRead<UiElement>
        + CanInsert<InputFocus>
        + CanUpdate<InputFocus>
        + CanUpdate<UiElement>
        + CanRead<UiInputState>
        + CanUpdate<UiInputState>
        + CanDelete<InputFocus>,
{
    // ── Text input ───────────────────────────────────────────────────────────
    let focused_id = ctx
        .current
        .tables
        .inputfocus()
        .get(0)
        .and_then(|f| f.focused_element.clone());

    if let Some(ref fid) = focused_id {
        // Apply character to focused element.
        if let Some(mut el) = ctx.current.tables.uielement().get(fid.clone()) {
            if el.is_input {
                let mut text = el.text.clone().unwrap_or_default();
                if new_buf.character == "\x08" {
                    // Backspace: remove last char, adjust cursor.
                    if el.cursor_pos > 0 {
                        let byte_pos = char_to_byte_pos(&text, el.cursor_pos as usize - 1);
                        let end = char_to_byte_pos(&text, el.cursor_pos as usize);
                        text.drain(byte_pos..end);
                        el.cursor_pos -= 1;
                    }
                } else {
                    // Insert character at cursor position.
                    let byte_pos = char_to_byte_pos(&text, el.cursor_pos as usize);
                    ctx.log(&new_buf.character);
                    text.insert_str(byte_pos, &new_buf.character);
                    el.cursor_pos += 1;
                }
                el.text = Some(text);
                let _ = ctx.current.tables.uielement().update(el);
            }
        }
    }
}
