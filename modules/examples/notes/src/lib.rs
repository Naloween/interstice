//! Notes: time travel from a UI (DESIGN §12.2).
//!
//! A front-end with no data of its own. The notes live in `hello-example`'s
//! `Notes` table, a logged table whose log is its history (§6.7): this app
//! reads the rows and the commit tree through its `hello-example` binding,
//! and edits them only by calling hello's reducers (`note`, `erase`, `undo`,
//! `redo`, `restore`). A click on a commit chip restores the table there;
//! the commits it leaves behind stay in the history as a branch.
//!
//! All input goes through `input.input_view`, so the app behaves the same on
//! its own and inside a desktop window.

use crate::bindings::{graphics::*, hello_example::*, input::*};
use interstice_sdk::{key_code::KeyCode, *};

interstice_module!(visibility: Public);

interstice_ui::ui_subsystem!();

use crate::ui::{AlignItems, HasUiElementEditHandle, LayoutDirection, Size, TextWrap, UiElement};

const INPUT_ID: &str = "note_input";
const LIST_ID: &str = "list";
const TIMELINE_ID: &str = "timeline";
const STATUS_ID: &str = "status";

const BG: (f32, f32, f32, f32) = (0.10, 0.10, 0.13, 1.0);
const CARD: (f32, f32, f32, f32) = (0.17, 0.18, 0.22, 1.0);
const FIELD: (f32, f32, f32, f32) = (0.13, 0.14, 0.18, 1.0);
const ACCENT: (f32, f32, f32, f32) = (0.27, 0.47, 0.87, 1.0);
const DANGER: (f32, f32, f32, f32) = (0.62, 0.25, 0.28, 1.0);
const CHIP: (f32, f32, f32, f32) = (0.22, 0.24, 0.30, 1.0);
const TEXT: (f32, f32, f32, f32) = (0.92, 0.92, 0.95, 1.0);
const MUTED: (f32, f32, f32, f32) = (0.60, 0.60, 0.66, 1.0);

/// UI bookkeeping (singleton): the signature of what the list and timeline
/// currently show (rebuilt only when it changes) and the left button last
/// frame (for click edges).
#[table(ephemeral)]
pub struct NotesView {
    #[primary_key]
    id: u32,
    signature: u64,
    was_down: bool,
}

#[reducer(on = "load")]
pub fn on_load<Caps>(ctx: ReducerContext<Caps>)
where
    Caps: CanInsert<ui::InputFocus> + CanUpdate<ui::InputFocus> + CanInsert<UiElement> + CanUpdate<UiElement>,
{
    ui::install(&ctx);
    let el = |e: UiElement| ui::create_element(&ctx, e);
    el(UiElement {
        id: "root".into(),
        width: Size::Grow,
        height: Size::Grow,
        padding: 18.0,
        gap: 12.0,
        background_color: BG,
        ..Default::default()
    });
    el(text("title", "root", 0, "Notes", 22.0, TEXT, true));
    el(text(
        "subtitle",
        "root",
        1,
        "Stored in hello-example's Notes table, whose log is its history. Undo, redo, or click a commit to restore it: what you leave stays as a branch.",
        13.0,
        MUTED,
        false,
    ));
    el(UiElement {
        id: "input_row".into(),
        parent: Some("root".into()),
        order: 2,
        width: Size::Grow,
        height: Size::Fixed(40.0),
        layout_direction: LayoutDirection::Row,
        gap: 8.0,
        ..Default::default()
    });
    el(UiElement {
        id: INPUT_ID.into(),
        parent: Some("input_row".into()),
        order: 0,
        width: Size::Grow,
        height: Size::Grow,
        padding: 10.0,
        background_color: FIELD,
        corner_radius: 6.0,
        border_width: 1.0,
        border_color: (0.32, 0.35, 0.45, 1.0),
        text: Some(String::new()),
        text_size: 15.0,
        text_color: TEXT,
        is_input: true,
        ..Default::default()
    });
    el(button("btn_add", "input_row", 1, "Add", ACCENT, 80.0));
    el(UiElement {
        id: LIST_ID.into(),
        parent: Some("root".into()),
        order: 3,
        width: Size::Grow,
        height: Size::Grow,
        gap: 6.0,
        scrollable_y: true,
        ..Default::default()
    });
    el(UiElement {
        id: "toolbar".into(),
        parent: Some("root".into()),
        order: 4,
        width: Size::Grow,
        height: Size::Fixed(34.0),
        layout_direction: LayoutDirection::Row,
        align_items: AlignItems::Center,
        gap: 8.0,
        ..Default::default()
    });
    el(button("btn_undo", "toolbar", 0, "Undo", CHIP, 80.0));
    el(button("btn_redo", "toolbar", 1, "Redo", CHIP, 80.0));
    el(text(STATUS_ID, "toolbar", 2, "", 13.0, MUTED, false));
    el(UiElement {
        id: TIMELINE_ID.into(),
        parent: Some("root".into()),
        order: 5,
        width: Size::Grow,
        height: Size::Fixed(36.0),
        layout_direction: LayoutDirection::Row,
        gap: 6.0,
        scrollable_x: true,
        ..Default::default()
    });
    ui::set_focus(&ctx, INPUT_ID);
}

#[reducer(on = "graphics.frametick.update")]
pub fn on_frame<Caps>(ctx: ReducerContext<Caps>, _prev: FrameTick, _tick: FrameTick) -> Result<(), String>
where
    Caps: CanAll<UiElement>
        + CanRead<ui::InputFocus>
        + CanInsert<ui::InputFocus>
        + CanUpdate<ui::InputFocus>
        + CanAll<ui::UiPointer>
        + CanAll<NotesView>
        + CanRead<Notes>,
{
    let hello = ctx.hello_example();
    let mut notes = hello.tables.notes().scan()?;
    notes.sort_by_key(|n| n.id);
    let history = hello.queries.note_history()?;

    // Rebuild the list and the timeline only when what they show changed.
    let signature = signature(&notes, &history);
    let prev = ctx.current.tables.notesview().get(0)?;
    if prev.as_ref().map(|v| v.signature) != Some(signature) {
        rebuild(&ctx, &notes, &history)?;
    }

    // Clicks: a press edge while the pointer is over us, resolved to the
    // element under it. Element ids encode the action.
    let view = ctx.input().queries.input_view()?;
    let was_down = prev.as_ref().is_some_and(|v| v.was_down);
    if view.left && !was_down && view.pointer_inside {
        if let Some(id) = ui::element_at(&ctx, view.position) {
            click(&ctx, &id, &notes)?;
        }
    }

    let row = NotesView { id: 0, signature, was_down: view.left };
    match prev {
        None => ctx.current.tables.notesview().insert(row).map(|_| ())?,
        Some(p) if p.signature != row.signature || p.was_down != row.was_down => {
            ctx.current.tables.notesview().update(row)?
        }
        Some(_) => {}
    }

    ui::render(&ctx)
}

/// Enter adds the note being typed (only while we have the keyboard).
#[reducer(on = "input.keystate.update")]
pub fn on_key<Caps>(ctx: ReducerContext<Caps>, prev: KeyState, new: KeyState) -> Result<(), String>
where
    Caps: CanAll<UiElement> + CanRead<Notes>,
{
    let enter = new.code == KeyCode::Enter as u32 || new.code == KeyCode::NumpadEnter as u32;
    if !enter || !new.pressed || prev.pressed {
        return Ok(());
    }
    if !ctx.input().queries.input_view()?.keyboard_focus {
        return Ok(());
    }
    let notes = ctx.hello_example().tables.notes().scan()?;
    add_note(&ctx, &notes)
}

fn click<Caps>(ctx: &ReducerContext<Caps>, id: &str, notes: &[Notes]) -> Result<(), String>
where
    Caps: CanAll<UiElement> + CanInsert<ui::InputFocus> + CanUpdate<ui::InputFocus>,
{
    let hello = ctx.hello_example();
    match id {
        "btn_add" => add_note(ctx, notes)?,
        "btn_undo" => hello.reducers.undo()?,
        "btn_redo" => hello.reducers.redo()?,
        INPUT_ID => ui::set_focus(ctx, INPUT_ID),
        _ => {
            if let Some(note) = id.strip_prefix("del_") {
                hello.reducers.erase(note.to_string())?;
            } else if let Some(commit) = id.strip_prefix("commit_") {
                hello.reducers.restore(commit.to_string())?;
            }
        }
    }
    Ok(())
}

/// Send the input's text to hello as a new note and clear the field.
fn add_note<Caps>(ctx: &ReducerContext<Caps>, notes: &[Notes]) -> Result<(), String>
where
    Caps: CanAll<UiElement>,
{
    let Some(mut input) = ctx.current.tables.uielement().get(INPUT_ID.to_string())? else {
        return Ok(());
    };
    let text = input.text.clone().unwrap_or_default();
    if text.trim().is_empty() {
        return Ok(());
    }
    let next = notes.iter().map(|n| n.id).max().unwrap_or(0) + 1;
    ctx.hello_example().reducers.note(next.to_string(), text.trim().to_string())?;
    input.text = Some(String::new());
    input.cursor_pos = 0;
    ctx.current.tables.uielement().update(input)
}

fn rebuild<Caps>(ctx: &ReducerContext<Caps>, notes: &[Notes], history: &NoteHistory) -> Result<(), String>
where
    Caps: CanAll<UiElement>,
{
    // Drop rows and chips that are no longer shown; upsert the rest.
    let keep_rows: Vec<String> = notes.iter().map(|n| format!("row_{}", n.id)).collect();
    let keep_chips: Vec<String> = std::iter::once(0)
        .chain(history.commits.iter().map(|c| c.id))
        .map(|id| format!("commit_{id}"))
        .collect();
    for el in ctx.current.tables.uielement().scan()? {
        let stale = match el.parent.as_deref() {
            Some(LIST_ID) => !keep_rows.contains(&el.id),
            Some(TIMELINE_ID) => !keep_chips.contains(&el.id),
            _ => false,
        };
        if stale {
            ui::delete_element(ctx, &el.id)?;
        }
    }

    for (i, n) in notes.iter().enumerate() {
        let row = format!("row_{}", n.id);
        ui::create_element(
            ctx,
            UiElement {
                id: row.clone(),
                parent: Some(LIST_ID.into()),
                order: i as u32,
                width: Size::Grow,
                height: Size::Fixed(40.0),
                layout_direction: LayoutDirection::Row,
                align_items: AlignItems::Center,
                padding: 6.0,
                gap: 8.0,
                background_color: CARD,
                corner_radius: 6.0,
                ..Default::default()
            },
        );
        ui::create_element(
            ctx,
            UiElement {
                width: Size::Grow,
                ..text(&format!("text_{}", n.id), &row, 0, &format!("{}.  {}", n.id, n.text), 15.0, TEXT, false)
            },
        );
        ui::create_element(ctx, button(&format!("del_{}", n.id), &row, 1, "Delete", DANGER, 70.0));
    }
    if notes.is_empty() {
        ui::create_element(ctx, text("row_empty", LIST_ID, 0, "No notes yet: type one above.", 14.0, MUTED, false));
    } else {
        let _ = ui::delete_element(ctx, "row_empty");
    }

    // Timeline chips: the base, then every commit by id. A commit whose
    // parent is not the one before it starts a branch: show where from.
    let mut prev_id = 0;
    for (i, id) in std::iter::once(0).chain(history.commits.iter().map(|c| c.id)).enumerate() {
        let parent = history.commits.iter().find(|c| c.id == id).map(|c| c.parent).unwrap_or(0);
        let label = match id {
            0 => "start".to_string(),
            _ if parent != prev_id => format!("#{id} (from #{parent})"),
            _ => format!("#{id}"),
        };
        prev_id = id;
        let head = id == history.head;
        let w = 18.0 + 8.0 * label.chars().count() as f32;
        let mut chip = button(&format!("commit_{id}"), TIMELINE_ID, i as u32, &label, if head { ACCENT } else { CHIP }, w);
        chip.height = Size::Fixed(30.0);
        ui::create_element(ctx, chip);
    }

    if let Some(mut status) = ctx.current.tables.uielement().get(STATUS_ID.to_string())? {
        let at = if history.head == 0 { "the start".to_string() } else { format!("commit #{}", history.head) };
        status.text = Some(format!("{} notes · at {at} · {} commits", notes.len(), history.commits.len()));
        ctx.current.tables.uielement().update(status)?;
    }
    Ok(())
}

/// FNV-1a over everything the list and the timeline show.
fn signature(notes: &[Notes], history: &NoteHistory) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for b in bytes {
            h = (h ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for n in notes {
        eat(&n.id.to_le_bytes());
        eat(n.text.as_bytes());
        eat(&[0]);
    }
    eat(&history.head.to_le_bytes());
    for c in &history.commits {
        eat(&c.id.to_le_bytes());
        eat(&c.parent.to_le_bytes());
    }
    h
}

fn text(id: &str, parent: &str, order: u32, body: &str, size: f32, color: (f32, f32, f32, f32), bold: bool) -> UiElement {
    UiElement {
        id: id.into(),
        parent: Some(parent.into()),
        order,
        width: Size::Grow,
        height: Size::Fit,
        text: Some(body.into()),
        text_size: size,
        text_color: color,
        text_bold: bold,
        text_wrap: TextWrap::Words,
        ..Default::default()
    }
}

fn button(id: &str, parent: &str, order: u32, label: &str, bg: (f32, f32, f32, f32), w: f32) -> UiElement {
    UiElement {
        id: id.into(),
        parent: Some(parent.into()),
        order,
        width: Size::Fixed(w),
        height: Size::Grow,
        padding: 8.0,
        background_color: bg,
        corner_radius: 6.0,
        text: Some(label.into()),
        text_size: 14.0,
        text_color: TEXT,
        text_align: 0.5,
        center_cross: true,
        ..Default::default()
    }
}
