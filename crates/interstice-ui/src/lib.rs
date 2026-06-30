//! Reusable UI layout/render engine for Interstice modules.
//!
//! The engine (layout, text, draw) is binding-agnostic and lives here once. Each
//! consuming module calls [`ui_subsystem!`] to paste a thin, module-local UI
//! subsystem — its own `#[table]` rows, element helpers, a key-input reducer and
//! a [`DrawTarget`] wired to that module's own graphics binding. Because every
//! module draws its OWN graphics layers, the compositor can route each module's
//! output to its own surface (a shared UI server cannot — all callers collapse
//! into one surface identity).

mod draw;
mod layout;
mod text;
mod types;

pub use draw::*;
pub use layout::*;
pub use text::*;
pub use types::*;

/// Emit a module-local UI subsystem into a `pub mod ui`.
///
/// Generates: `UiElement` (public) + `InputFocus` (ephemeral) tables, element
/// helpers (`install`, `create_element`, `update_element`, `delete_element`,
/// `clear_elements`, `set_focus`, `clear_focus`), a `render` helper that draws
/// into the module's own `"ui"` layer (sized from the module's OWN surface via
/// `surface_info`), and a `input.textinputbuffer.update` reducer for text entry.
///
/// Requires the consuming module to have `graphics` and `input` bindings.
#[macro_export]
macro_rules! ui_subsystem {
    () => {
        pub mod ui {
            use crate::bindings::graphics::*;
            use crate::bindings::input::*;
            use interstice_sdk::*;

            // Layout primitives come straight from the engine so element literals
            // read identically to the old shared-module API.
            pub use interstice_ui::{
                AlignItems, FontStyle, JustifyContent, LayoutDirection, Position, Size, TextSpan,
                TextWrap,
            };

            /// The retained UI tree for this module. Identical field set to
            /// [`interstice_ui::UiElement`]; converted via [`into_lib`] before layout.
            #[table(public)]
            pub struct UiElement {
                #[primary_key]
                pub id: String,
                pub parent: Option<String>,
                pub order: u32,
                pub width: Size,
                pub height: Size,
                pub layout_direction: LayoutDirection,
                pub justify_content: JustifyContent,
                pub align_items: AlignItems,
                pub position: Position,
                pub pos_left: Option<f32>,
                pub pos_top: Option<f32>,
                pub pos_right: Option<f32>,
                pub pos_bottom: Option<f32>,
                pub gap: f32,
                pub padding: f32,
                pub margin: f32,
                pub padding_sides: Option<(f32, f32, f32, f32)>,
                pub margin_sides: Option<(f32, f32, f32, f32)>,
                pub background_color: (f32, f32, f32, f32),
                pub corner_radius: f32,
                pub border_width: f32,
                pub border_color: (f32, f32, f32, f32),
                pub text: Option<String>,
                pub text_size: f32,
                pub text_color: (f32, f32, f32, f32),
                pub text_wrap: TextWrap,
                pub text_bold: bool,
                pub text_italic: bool,
                pub spans: Vec<TextSpan>,
                pub text_align: f32,
                /// Explicit CSS `line-height` in px; `<= 0.0` ⇒ natural font height.
                pub line_height: f32,
                pub image: Option<String>,
                pub is_input: bool,
                pub cursor_pos: u32,
                pub scrollable_x: bool,
                pub scrollable_y: bool,
                pub scroll_x: f32,
                pub scroll_y: f32,
                pub visible: bool,
            }

            /// Neutral defaults so element literals can use `..Default::default()`
            /// and stay source-compatible as the engine grows new layout fields.
            impl Default for UiElement {
                fn default() -> Self {
                    UiElement {
                        id: String::new(),
                        parent: None,
                        order: 0,
                        width: Size::Fit,
                        height: Size::Fit,
                        layout_direction: LayoutDirection::Column,
                        justify_content: JustifyContent::Start,
                        align_items: AlignItems::Stretch,
                        position: Position::Static,
                        pos_left: None,
                        pos_top: None,
                        pos_right: None,
                        pos_bottom: None,
                        gap: 0.0,
                        padding: 0.0,
                        margin: 0.0,
                        padding_sides: None,
                        margin_sides: None,
                        background_color: (0.0, 0.0, 0.0, 0.0),
                        corner_radius: 0.0,
                        border_width: 0.0,
                        border_color: (0.0, 0.0, 0.0, 0.0),
                        text: None,
                        text_size: 0.0,
                        text_color: (0.0, 0.0, 0.0, 0.0),
                        text_wrap: TextWrap::None,
                        text_bold: false,
                        text_italic: false,
                        spans: Vec::new(),
                        text_align: 0.0,
                        line_height: 0.0,
                        image: None,
                        is_input: false,
                        cursor_pos: 0,
                        scrollable_x: false,
                        scrollable_y: false,
                        scroll_x: 0.0,
                        scroll_y: 0.0,
                        visible: true,
                    }
                }
            }

            /// Which element currently holds keyboard focus.
            #[table(ephemeral)]
            pub struct InputFocus {
                #[primary_key]
                pub id: u32,
                pub focused_element: Option<String>,
            }

            /// The single swapchain/surface-facing layer this module draws into.
            pub const UI_LAYER: &str = "ui";
            pub const UI_LAYER_Z: i32 = 100;

            /// A dedicated top-most layer for the cursor. Within a single layer
            /// the graphics renderer composites images in a pass *after* all
            /// immediate primitives (rects/text/circles), so a cursor drawn as a
            /// circle into `UI_LAYER` would be hidden behind any page image under
            /// it. Drawing the cursor into its own higher-`z` layer keeps it on
            /// top of everything the UI layer renders.
            pub const UI_CURSOR_LAYER: &str = "ui_cursor";
            pub const UI_CURSOR_LAYER_Z: i32 = 1000;

            /// Move a scanned row into the engine's element type. `scan()` already
            /// hands us owned rows, so converting by MOVE avoids a second deep
            /// clone of every id/text/span on a multi-thousand-element page — the
            /// render/hit-test paths only need the converted copy, never the row.
            fn into_lib(e: UiElement) -> interstice_ui::UiElement {
                interstice_ui::UiElement {
                    id: e.id,
                    parent: e.parent,
                    order: e.order,
                    width: e.width,
                    height: e.height,
                    layout_direction: e.layout_direction,
                    justify_content: e.justify_content,
                    align_items: e.align_items,
                    position: e.position,
                    pos_left: e.pos_left,
                    pos_top: e.pos_top,
                    pos_right: e.pos_right,
                    pos_bottom: e.pos_bottom,
                    gap: e.gap,
                    padding: e.padding,
                    margin: e.margin,
                    padding_sides: e.padding_sides,
                    margin_sides: e.margin_sides,
                    background_color: e.background_color,
                    corner_radius: e.corner_radius,
                    border_width: e.border_width,
                    border_color: e.border_color,
                    text: e.text,
                    text_size: e.text_size,
                    text_color: e.text_color,
                    text_wrap: e.text_wrap,
                    text_bold: e.text_bold,
                    text_italic: e.text_italic,
                    spans: e.spans,
                    text_align: e.text_align,
                    line_height: e.line_height,
                    image: e.image,
                    is_input: e.is_input,
                    cursor_pos: e.cursor_pos,
                    scrollable_x: e.scrollable_x,
                    scrollable_y: e.scrollable_y,
                    scroll_x: e.scroll_x,
                    scroll_y: e.scroll_y,
                    visible: e.visible,
                }
            }

            /// [`interstice_ui::DrawTarget`] forwarding into this module's own
            /// graphics layer.
            struct GraphicsTarget<'a, Caps> {
                ctx: &'a ReducerContext<Caps>,
                layer: String,
            }

            impl<'a, Caps> interstice_ui::DrawTarget for GraphicsTarget<'a, Caps> {
                fn rect(
                    &mut self,
                    x: f32,
                    y: f32,
                    w: f32,
                    h: f32,
                    color: (f32, f32, f32, f32),
                    filled: bool,
                    stroke_width: f32,
                    corner_radius: Option<f32>,
                ) {
                    let (r, g, b, a) = color;
                    let _ = self.ctx.graphics().reducers.draw_rect(
                        self.layer.clone(),
                        Rect { x, y, w, h },
                        Color { r, g, b, a },
                        filled,
                        stroke_width,
                        corner_radius,
                    );
                }
                fn text(
                    &mut self,
                    content: &str,
                    x: f32,
                    y: f32,
                    size: f32,
                    style: interstice_ui::FontStyle,
                    color: (f32, f32, f32, f32),
                ) {
                    let (r, g, b, a) = color;
                    // The graphics `draw_text` ABI carries a `font` selector; we
                    // reuse it to name the weight/slant so no binding changes.
                    let font = match (style.bold, style.italic) {
                        (false, false) => None,
                        (true, false) => Some("bold".to_string()),
                        (false, true) => Some("italic".to_string()),
                        (true, true) => Some("bold-italic".to_string()),
                    };
                    let _ = self.ctx.graphics().reducers.draw_text(
                        self.layer.clone(),
                        content.to_string(),
                        Vec2 { x, y },
                        size,
                        Color { r, g, b, a },
                        font,
                    );
                }
                fn circle(
                    &mut self,
                    x: f32,
                    y: f32,
                    radius: f32,
                    color: (f32, f32, f32, f32),
                    filled: bool,
                    stroke_width: f32,
                ) {
                    let (r, g, b, a) = color;
                    let _ = self.ctx.graphics().reducers.draw_circle(
                        self.layer.clone(),
                        Vec2 { x, y },
                        radius,
                        Color { r, g, b, a },
                        filled,
                        stroke_width,
                    );
                }
                fn image(
                    &mut self,
                    local_id: &str,
                    x: f32,
                    y: f32,
                    w: f32,
                    h: f32,
                    u0: f32,
                    v0: f32,
                    u1: f32,
                    v1: f32,
                ) {
                    let _ = self.ctx.graphics().reducers.draw_image(
                        self.layer.clone(),
                        local_id.to_string(),
                        Rect { x, y, w, h },
                        Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
                        Rect { x: u0, y: v0, w: u1 - u0, h: v1 - v0 },
                    );
                }
            }

            // ── Lifecycle ────────────────────────────────────────────────────

            /// Create the UI layer. Call from `on_load`. The `InputFocus` row is
            /// created lazily by `set_focus`/`clear_focus` (NOT seeded here): a
            /// row inserted in this run would not be visible to a `get()` in the
            /// same run, so seeding + reading focus in one `load` run would fail.
            pub fn install<Caps>(ctx: &ReducerContext<Caps>) {
                let _ = ctx
                    .graphics()
                    .reducers
                    .create_layer(UI_LAYER.to_string(), UI_LAYER_Z, false);
                let _ = ctx
                    .graphics()
                    .reducers
                    .create_layer(UI_CURSOR_LAYER.to_string(), UI_CURSOR_LAYER_Z, false);
            }

            // ── Element helpers ──────────────────────────────────────────────

            // Idempotent upsert. `insert` succeeds the first time the module
            // runs; if the module is unloaded and loaded again the runtime keeps
            // its persisted `UiElement` rows, so `on_load`'s inserts would hit a
            // unique-constraint violation — fall back to `update` in that case so
            // a reload simply re-establishes the same element. We don't read
            // first because a same-run insert wouldn't be visible (see the
            // write-visibility note on `set_focus`).
            pub fn create_element<Caps>(ctx: &ReducerContext<Caps>, element: UiElement)
            where
                Caps: CanInsert<UiElement> + CanUpdate<UiElement>,
            {
                if ctx.current.tables.uielement().insert(element.clone()).is_err() {
                    if let Err(err) = ctx.current.tables.uielement().update(element) {
                        ctx.log(&format!("ui: create_element failed: {err}"));
                    }
                }
            }

            pub fn update_element<Caps>(ctx: &ReducerContext<Caps>, element: UiElement)
            where
                Caps: CanUpdate<UiElement>,
            {
                if let Err(err) = ctx.current.tables.uielement().update(element) {
                    ctx.log(&format!("ui: update_element failed: {err}"));
                }
            }

            fn delete_recursive<Caps>(ctx: &ReducerContext<Caps>, id: &str)
            where
                Caps: CanRead<UiElement> + CanDelete<UiElement>,
            {
                let children: Vec<String> = ctx
                    .current
                    .tables
                    .uielement()
                    .scan()
                    .into_iter()
                    .filter(|e| e.parent.as_deref() == Some(id))
                    .map(|e| e.id)
                    .collect();
                for child_id in children {
                    delete_recursive(ctx, &child_id);
                }
                let _ = ctx.current.tables.uielement().delete(id.to_string());
            }

            pub fn delete_element<Caps>(ctx: &ReducerContext<Caps>, id: &str)
            where
                Caps: CanRead<UiElement> + CanDelete<UiElement>,
            {
                delete_recursive(ctx, id);
            }

            pub fn clear_elements<Caps>(ctx: &ReducerContext<Caps>)
            where
                Caps: CanRead<UiElement> + CanDelete<UiElement>,
            {
                for el in ctx.current.tables.uielement().scan() {
                    let _ = ctx.current.tables.uielement().delete(el.id);
                }
            }

            // Focus is a singleton (id 0). These upsert WITHOUT reading first:
            // `insert` succeeds the first time; on the next call the row already
            // exists (committed) so insert fails and we `update`. A read-then-write
            // would miss a same-run insert (see write-visibility note on `install`).
            pub fn set_focus<Caps>(ctx: &ReducerContext<Caps>, id: &str)
            where
                Caps: CanInsert<InputFocus> + CanUpdate<InputFocus>,
            {
                if ctx
                    .current
                    .tables
                    .inputfocus()
                    .insert(InputFocus {
                        id: 0,
                        focused_element: Some(id.to_string()),
                    })
                    .is_err()
                {
                    let _ = ctx.current.tables.inputfocus().update(InputFocus {
                        id: 0,
                        focused_element: Some(id.to_string()),
                    });
                }
            }

            pub fn clear_focus<Caps>(ctx: &ReducerContext<Caps>)
            where
                Caps: CanInsert<InputFocus> + CanUpdate<InputFocus>,
            {
                if ctx
                    .current
                    .tables
                    .inputfocus()
                    .insert(InputFocus {
                        id: 0,
                        focused_element: None,
                    })
                    .is_err()
                {
                    let _ = ctx.current.tables.inputfocus().update(InputFocus {
                        id: 0,
                        focused_element: None,
                    });
                }
            }

            /// Hit-test: id of the innermost UI element under `cursor`, sized
            /// from this module's own surface. Used by apps to resolve a click to
            /// an element (e.g. a browser link). Mirrors [`render`]'s layout.
            pub fn element_at<Caps>(ctx: &ReducerContext<Caps>, cursor: (f32, f32)) -> Option<String>
            where
                Caps: CanRead<UiElement>,
            {
                let info = ctx.graphics().queries.surface_info().ok()?;
                let (sw, sh) = (info.width as f32, info.height as f32);
                if sw < 1.0 || sh < 1.0 {
                    return None;
                }
                let all: Vec<interstice_ui::UiElement> = ctx
                    .current
                    .tables
                    .uielement()
                    .scan()
                    .into_iter()
                    .map(into_lib)
                    .collect();
                interstice_ui::find_element_at(&all, sw, sh, cursor)
            }

            /// Hit-test for inline links: the `href` of the link span under
            /// `cursor`, or `None` if the click landed on plain text. Mirrors
            /// [`render`]'s span layout. Use for clickable inline text (e.g. a
            /// browser anchor wrapped mid-paragraph).
            pub fn link_at<Caps>(ctx: &ReducerContext<Caps>, cursor: (f32, f32)) -> Option<String>
            where
                Caps: CanRead<UiElement>,
            {
                let info = ctx.graphics().queries.surface_info().ok()?;
                let (sw, sh) = (info.width as f32, info.height as f32);
                if sw < 1.0 || sh < 1.0 {
                    return None;
                }
                let all: Vec<interstice_ui::UiElement> = ctx
                    .current
                    .tables
                    .uielement()
                    .scan()
                    .into_iter()
                    .map(into_lib)
                    .collect();
                interstice_ui::link_at(&all, sw, sh, cursor)
            }

            // ── Per-frame render ─────────────────────────────────────────────

            /// Lay out and draw this module's UI tree into its own layer. Sizes
            /// from the module's OWN surface (`surface_info`), so it composites
            /// 1:1 whether rendering to the swapchain or to an offscreen surface.
            pub fn render<Caps>(ctx: &ReducerContext<Caps>)
            where
                Caps: CanRead<UiElement>
                    + CanUpdate<UiElement>
                    + CanRead<InputFocus>
                    + CanRead<MouseState>,
            {
                let info = match ctx.graphics().queries.surface_info() {
                    Ok(info) => info,
                    Err(_) => return,
                };
                let (sw, sh) = (info.width as f32, info.height as f32);
                if sw < 1.0 || sh < 1.0 {
                    return;
                }

                // Scroll: nudge the innermost scrollable under the cursor.
                //
                // `wheel_delta` from the input authority is a running *cumulative*
                // total (it's never reset at the source), so we keep the last value
                // we saw and act only on the per-frame increment — otherwise every
                // frame would re-apply the whole history and the page would fly off.
                // The wasm instance persists between frame calls, so a module-local
                // static is the right place for this tiny bit of bookkeeping (no
                // table / capability needed).
                if let Some(mouse) = ctx.input().tables.mousestate().get(0) {
                    static mut PREV_WHEEL: (f32, f32) = (0.0, 0.0);
                    let (cum_x, cum_y) = mouse.wheel_delta;
                    let (prev_x, prev_y) = unsafe { PREV_WHEEL };
                    unsafe { PREV_WHEEL = (cum_x, cum_y) };
                    let (wx, wy) = (cum_x - prev_x, cum_y - prev_y);
                    if wx != 0.0 || wy != 0.0 {
                        let cursor = mouse.position;
                        let all: Vec<interstice_ui::UiElement> = ctx
                            .current
                            .tables
                            .uielement()
                            .scan()
                            .into_iter()
                            .map(into_lib)
                            .collect();
                        let full = (0.0, 0.0, sw, sh);
                        let mut best: Option<(String, bool, bool)> = None;
                        for root in all.iter().filter(|e| e.parent.is_none()) {
                            interstice_ui::find_scrollable_at(
                                &all, root, cursor, 0.0, 0.0, sw, sh, full, &mut best,
                            );
                        }
                        if let Some((sid, sx, sy)) = best {
                            if let Some(mut el) = ctx.current.tables.uielement().get(sid) {
                                // Wheel deltas arrive already normalised to pixels by
                                // the input authority. Touchpads (PixelDelta) report
                                // large raw pixel deltas per gesture, so we damp them
                                // below 1:1 to keep two-finger scrolling controllable.
                                // (Tweak this factor to taste for faster/slower.)
                                // Direction: SUBTRACT the delta so a downward wheel/
                                // two-finger drag moves the content up (view scrolls
                                // down) — the conventional, non-"natural" direction.
                                const SCROLL_SPEED: f32 = 0.30;
                                if sx {
                                    el.scroll_x = (el.scroll_x - wx * SCROLL_SPEED).max(0.0);
                                }
                                if sy {
                                    el.scroll_y = (el.scroll_y - wy * SCROLL_SPEED).max(0.0);
                                }
                                let _ = ctx.current.tables.uielement().update(el);
                            }
                        }
                    }
                }

                let focused = ctx
                    .current
                    .tables
                    .inputfocus()
                    .get(0)
                    .and_then(|f| f.focused_element);
                let all: Vec<interstice_ui::UiElement> = ctx
                    .current
                    .tables
                    .uielement()
                    .scan()
                    .into_iter()
                    .map(into_lib)
                    .collect();
                let mut target = GraphicsTarget {
                    ctx,
                    layer: UI_LAYER.to_string(),
                };
                interstice_ui::render(&all, sw, sh, focused.as_deref(), &mut target);

                // Draw the cursor only when rendering straight to the swapchain
                // (surface 0). When assigned to an offscreen surface (inside a
                // desktop window), the compositor owns the global cursor — drawing
                // our own would trap it inside the window.
                if info.id == 0 {
                    if let Some(mouse) = ctx.input().tables.mousestate().get(0) {
                        let (mx, my) = mouse.position;
                        // Draw into the dedicated cursor layer so it stays above
                        // page images (which the renderer composites after the
                        // UI layer's immediate primitives — see UI_CURSOR_LAYER).
                        let mut cursor_target = GraphicsTarget {
                            ctx,
                            layer: UI_CURSOR_LAYER.to_string(),
                        };
                        interstice_ui::draw_cursor(&mut cursor_target, mx, my);
                    }
                }
            }

            // ── Text input ───────────────────────────────────────────────────

            #[reducer(on = "input.textinputbuffer.update")]
            pub fn ui_on_key<Caps>(
                ctx: ReducerContext<Caps>,
                _previous_buf: TextInputBuffer,
                new_buf: TextInputBuffer,
            ) where
                Caps: CanRead<InputFocus> + CanRead<UiElement> + CanUpdate<UiElement>,
            {
                let focused_id = ctx
                    .current
                    .tables
                    .inputfocus()
                    .get(0)
                    .and_then(|f| f.focused_element.clone());

                if let Some(ref fid) = focused_id {
                    if let Some(mut el) = ctx.current.tables.uielement().get(fid.clone()) {
                        if el.is_input {
                            let mut text = el.text.clone().unwrap_or_default();
                            if new_buf.character == "\x08" {
                                if el.cursor_pos > 0 {
                                    let byte_pos = interstice_ui::char_to_byte_pos(
                                        &text,
                                        el.cursor_pos as usize - 1,
                                    );
                                    let end = interstice_ui::char_to_byte_pos(
                                        &text,
                                        el.cursor_pos as usize,
                                    );
                                    text.drain(byte_pos..end);
                                    el.cursor_pos -= 1;
                                }
                            } else {
                                let byte_pos = interstice_ui::char_to_byte_pos(
                                    &text,
                                    el.cursor_pos as usize,
                                );
                                text.insert_str(byte_pos, &new_buf.character);
                                el.cursor_pos += 1;
                            }
                            el.text = Some(text);
                            let _ = ctx.current.tables.uielement().update(el);
                        }
                    }
                }
            }
        }
    };
}
