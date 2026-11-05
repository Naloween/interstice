use crate::bindings::{graphics::*, input::*};
use interstice_sdk::*;
use interstice_ui::theme::{self as th, Rgba};

interstice_module!(visibility: Public);

// The UI demo: a sheet of the brand's components (brand/BRAND.md §7), built
// with `interstice-ui` from `interstice_ui::theme` alone.
//
// `ui_subsystem!` pastes a module-local UI subsystem (tables, helpers, render,
// key reducer) wired to this module's own graphics/input bindings. Everything
// draws into our OWN layers, so the desktop compositor can route us to our
// own surface.
interstice_ui::ui_subsystem!();

use crate::ui::{AlignItems, LayoutDirection, Size, TextWrap, UiElement};

const PRIMARY_ID: &str = "btn_primary";
const INPUT_ID: &str = "input";
const LIST_ROWS: [(&str, &str); 3] = [
    ("ui-example", "this window"),
    ("hello-example", "the notes backend"),
    ("graphics", "layers, text, surfaces"),
];

const CLEAR: Rgba = (0.0, 0.0, 0.0, 0.0);

#[reducer(on = "load")]
pub fn on_load<Caps>(ctx: ReducerContext<Caps>)
where
    Caps: CanInsert<ui::InputFocus> + CanUpdate<ui::InputFocus> + CanInsert<ui::UiElement> + CanUpdate<ui::UiElement>,
{
    ui::install(&ctx);
    let el = |e: UiElement| ui::create_element(&ctx, e);

    el(UiElement {
        id: "root".into(),
        width: Size::Grow,
        height: Size::Grow,
        padding: 32.0,
        gap: 14.0,
        background_color: th::CHARCOAL,
        scrollable_y: true,
        ..Default::default()
    });
    el(UiElement { text_wrap: TextWrap::None, ..text("kicker", "root", 0, &th::label("Interstice UI"), th::TEXT_CAPTION, th::GRAY) });
    el(text("title", "root", 1, "Components", th::TEXT_HEADING, th::IVORY));
    el(text(
        "subtitle",
        "root",
        2,
        "Every colour, size and radius on this sheet comes from interstice_ui::theme, the brand in code.",
        th::TEXT_BODY,
        th::MUTED,
    ));

    // Colour.
    section(&ctx, "colour", 3, "Colour");
    el(row("swatches", "root", 4, 12.0));
    let swatches: [(&str, Rgba); 8] = [
        ("ink", th::INK),
        ("ground", th::GROUND),
        ("charcoal", th::CHARCOAL),
        ("raised", th::RAISED),
        ("line", th::LINE),
        ("gray", th::GRAY),
        ("ivory", th::IVORY),
        ("amber", th::AMBER),
    ];
    for (i, (name, c)) in swatches.iter().enumerate() {
        let id = format!("sw_{name}");
        el(UiElement {
            id: id.clone(),
            parent: Some("swatches".into()),
            order: i as u32,
            width: Size::Fixed(72.0),
            height: Size::Fit,
            gap: 6.0,
            ..Default::default()
        });
        el(UiElement {
            id: format!("{id}_box"),
            parent: Some(id.clone()),
            order: 0,
            width: Size::Grow,
            height: Size::Fixed(40.0),
            background_color: *c,
            corner_radius: th::RADIUS_CONTROL,
            border_width: 1.0,
            border_color: th::LINE,
            ..Default::default()
        });
        el(UiElement { text_mono: true, ..text(&format!("{id}_name"), &id, 1, name, th::TEXT_CAPTION, th::GRAY) });
    }

    // Type.
    section(&ctx, "type", 5, "Type");
    el(text("type_heading", "root", 6, "Heading, 22 px", th::TEXT_HEADING, th::IVORY));
    el(text("type_reading", "root", 7, "Reading body, 15 px: a space for things to interact.", th::TEXT_READING, th::IVORY));
    el(text("type_ui", "root", 8, "UI body, 13 px, for controls and secondary lines.", th::TEXT_BODY, th::MUTED));
    el(UiElement {
        text_mono: true,
        ..text("type_mono", "root", 9, "IBM Plex Mono 12: 18:04:22 · tx 4096 · 0x2a", th::TEXT_SMALL, th::MUTED)
    });

    // Controls.
    section(&ctx, "controls", 10, "Controls");
    el(UiElement { height: Size::Fixed(36.0), ..row("buttons", "root", 11, 10.0) });
    el(button(PRIMARY_ID, "buttons", 0, "Click me", Kind::Primary, false, 120.0));
    el(button("btn_secondary", "buttons", 1, "Secondary", Kind::Secondary, false, 104.0));
    el(button("btn_quiet", "buttons", 2, "Quiet", Kind::Quiet, false, 72.0));
    el(button("btn_danger", "buttons", 3, "Delete", Kind::Destructive, false, 80.0));
    el(UiElement {
        id: INPUT_ID.into(),
        parent: Some("root".into()),
        order: 12,
        width: Size::Fixed(420.0),
        height: Size::Fixed(38.0),
        padding: 10.0,
        background_color: th::INK,
        corner_radius: th::RADIUS_CONTROL,
        border_width: 1.0,
        border_color: th::INPUT_BORDER,
        text: Some("Type here".into()),
        cursor_pos: 9,
        text_size: th::TEXT_BODY,
        text_color: th::IVORY,
        is_input: true,
        ..Default::default()
    });

    // List and chips.
    section(&ctx, "list", 13, "List");
    el(UiElement { id: "list".into(), parent: Some("root".into()), order: 14, width: Size::Fixed(420.0), height: Size::Fit, ..Default::default() });
    for (i, _) in LIST_ROWS.iter().enumerate() {
        list_row(&ctx, i, i == 0);
    }
    el(UiElement { height: Size::Fixed(26.0), ..row("chips", "root", 15, 6.0) });
    for (i, (label, head)) in [("start", false), ("#9", false), ("#12", false), ("#18", true)].iter().enumerate() {
        el(chip(&format!("chip_{i}"), i as u32, label, *head));
    }
    ui::set_focus(&ctx, INPUT_ID);
}

/// Click count, the selected list row, hover, and the left button last frame.
#[table(ephemeral)]
pub struct DemoState {
    #[primary_key]
    id: u32,
    clicks: u32,
    selected: u32,
    hovered: String,
    was_down: bool,
}

#[reducer(on = "graphics.frametick.update")]
pub fn on_frame<Caps>(ctx: ReducerContext<Caps>, _prev: FrameTick, _tick: FrameTick) -> Result<(), String>
where
    Caps: CanRead<ui::UiElement>
        + CanInsert<ui::UiElement>
        + CanUpdate<ui::UiElement>
        + CanRead<ui::InputFocus>
        + CanAll<ui::UiPointer>
        + CanAll<DemoState>,
{
    // The pointer as routed to us: surface-local inside a desktop window.
    let view = ctx.input().queries.input_view()?;
    let over = if view.pointer_inside { ui::element_at(&ctx, view.position).unwrap_or_default() } else { String::new() };
    // A hit on a row's label counts as the row.
    let over = over.strip_suffix("_name").or(over.strip_suffix("_note")).map(str::to_string).unwrap_or(over);

    let t = ctx.current.tables.demostate();
    let prev = t.get(0)?;
    let mut s = prev
        .clone()
        .unwrap_or(DemoState { id: 0, clicks: 0, selected: 0, hovered: String::new(), was_down: false });
    let clicked = view.left && !s.was_down;
    if clicked && over == PRIMARY_ID {
        s.clicks += 1;
    }
    if clicked {
        if let Some(i) = over.strip_prefix("row_").and_then(|n| n.parse::<u32>().ok()) {
            s.selected = i;
        }
    }
    s.was_down = view.left;
    s.hovered = over;

    let changed = prev.as_ref().is_none_or(|p| p.clicks != s.clicks || p.selected != s.selected || p.hovered != s.hovered);
    if changed {
        let label = if s.clicks == 0 { "Click me".to_string() } else { format!("Clicked {}×", s.clicks) };
        ui::update_element(&ctx, button(PRIMARY_ID, "buttons", 0, &label, Kind::Primary, s.hovered == PRIMARY_ID, 120.0));
        for (id, label, kind, order, w) in [
            ("btn_secondary", "Secondary", Kind::Secondary, 1, 104.0),
            ("btn_quiet", "Quiet", Kind::Quiet, 2, 72.0),
            ("btn_danger", "Delete", Kind::Destructive, 3, 80.0),
        ] {
            ui::update_element(&ctx, button(id, "buttons", order, label, kind, s.hovered == id, w));
        }
        for i in 0..LIST_ROWS.len() {
            list_row(&ctx, i, i as u32 == s.selected);
        }
    }
    match prev {
        None => t.insert(s).map(|_| ())?,
        Some(p) if p.was_down != s.was_down || changed => t.update(s)?,
        Some(_) => {}
    }

    // Lay out + draw our UI tree into our own layer/surface.
    ui::render(&ctx)
}

// ── Components ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum Kind {
    Primary,
    Secondary,
    Quiet,
    Destructive,
}

fn text(id: &str, parent: &str, order: u32, body: &str, size: f32, color: Rgba) -> UiElement {
    UiElement {
        id: id.into(),
        parent: Some(parent.into()),
        order,
        width: Size::Grow,
        height: Size::Fit,
        text: Some(body.into()),
        text_size: size,
        text_color: color,
        text_wrap: TextWrap::Words,
        ..Default::default()
    }
}

fn row(id: &str, parent: &str, order: u32, gap: f32) -> UiElement {
    UiElement {
        id: id.into(),
        parent: Some(parent.into()),
        order,
        width: Size::Grow,
        height: Size::Fit,
        layout_direction: LayoutDirection::Row,
        align_items: AlignItems::Center,
        gap,
        ..Default::default()
    }
}

/// A section header in the board's style: an uppercase label, then a hairline.
fn section<Caps>(ctx: &ReducerContext<Caps>, id: &str, order: u32, label: &str)
where
    Caps: CanInsert<ui::UiElement> + CanUpdate<ui::UiElement>,
{
    let rid = format!("sec_{id}");
    let mut r = row(&rid, "root", order, 14.0);
    r.margin_sides = Some((14.0, 0.0, 2.0, 0.0));
    ui::create_element(ctx, r);
    let caption = th::label(label);
    let w = interstice_ui::text_width(&caption, th::TEXT_CAPTION, interstice_ui::FontStyle::REGULAR).ceil() + 2.0;
    ui::create_element(
        ctx,
        UiElement {
            width: Size::Fixed(w),
            text_wrap: TextWrap::None,
            ..text(&format!("{rid}_l"), &rid, 0, &caption, th::TEXT_CAPTION, th::GRAY)
        },
    );
    ui::create_element(
        ctx,
        UiElement {
            id: format!("{rid}_rule"),
            parent: Some(rid.clone()),
            order: 1,
            width: Size::Grow,
            height: Size::Fixed(1.0),
            background_color: th::LINE,
            ..Default::default()
        },
    );
}

/// Buttons sit in the 36 px `buttons` row; the label is centred in it.
fn button(id: &str, parent: &str, order: u32, label: &str, kind: Kind, hover: bool, w: f32) -> UiElement {
    let row_h = 36.0;
    let (bg, fg, border) = match (kind, hover) {
        (Kind::Primary, false) => (th::AMBER, th::INK, CLEAR),
        (Kind::Primary, true) => (th::AMBER_HI, th::INK, CLEAR),
        (Kind::Secondary, h) => (if h { th::mix(th::RAISED, th::IVORY, 0.05) } else { th::RAISED }, th::IVORY, th::LINE),
        (Kind::Quiet, h) => (CLEAR, if h { th::IVORY } else { th::MUTED }, CLEAR),
        (Kind::Destructive, h) => (if h { th::alpha(th::DANGER, 0.14) } else { th::RAISED }, th::DANGER, th::LINE),
    };
    UiElement {
        id: id.into(),
        parent: Some(parent.into()),
        order,
        width: Size::Fixed(w),
        height: Size::Grow,
        // The engine puts text at the top of the content box: pad the top so
        // the label sits centred in the row's height.
        padding_sides: Some((((row_h - interstice_ui::text_line_height(th::TEXT_BODY, interstice_ui::FontStyle::REGULAR)) / 2.0).floor(), 8.0, 0.0, 8.0)),
        background_color: bg,
        corner_radius: th::RADIUS_CONTROL,
        border_width: if border.3 > 0.0 { 1.0 } else { 0.0 },
        border_color: border,
        text: Some(label.into()),
        text_size: th::TEXT_BODY,
        text_color: fg,
        text_bold: matches!(kind, Kind::Primary),
        text_align: 0.5,
        center_cross: true,
        ..Default::default()
    }
}

/// A 30 px list row; the selected one gets the amber tint and a 2 px bar.
fn list_row<Caps>(ctx: &ReducerContext<Caps>, i: usize, selected: bool)
where
    Caps: CanInsert<ui::UiElement> + CanUpdate<ui::UiElement>,
{
    let (name, note) = LIST_ROWS[i];
    let id = format!("row_{i}");
    ui::create_element(
        ctx,
        UiElement {
            id: id.clone(),
            parent: Some("list".into()),
            order: i as u32,
            width: Size::Grow,
            height: Size::Fixed(32.0),
            layout_direction: LayoutDirection::Row,
            align_items: AlignItems::Center,
            gap: 10.0,
            padding_sides: Some((0.0, 10.0, 0.0, 0.0)),
            background_color: if selected { th::SELECTED_BG } else { CLEAR },
            corner_radius: th::RADIUS_CHIP,
            ..Default::default()
        },
    );
    ui::create_element(
        ctx,
        UiElement {
            id: format!("{id}_bar"),
            parent: Some(id.clone()),
            order: 0,
            width: Size::Fixed(2.0),
            height: Size::Fixed(18.0),
            background_color: if selected { th::AMBER } else { CLEAR },
            ..Default::default()
        },
    );
    let name_c = if selected { th::IVORY } else { th::MUTED };
    ui::create_element(ctx, UiElement { width: Size::Fixed(150.0), ..text(&format!("{id}_name"), &id, 1, name, th::TEXT_BODY, name_c) });
    ui::create_element(ctx, text(&format!("{id}_note"), &id, 2, note, th::TEXT_BODY, th::GRAY));
}

/// A commit chip: mono, hairline border; the head is lit.
fn chip(id: &str, order: u32, label: &str, head: bool) -> UiElement {
    let w = 20.0 + interstice_ui::text_width(label, th::TEXT_CAPTION, interstice_ui::FontStyle::MONO);
    let (fg, border) = if head { (th::AMBER, th::alpha(th::AMBER, 0.7)) } else { (th::MUTED, th::LINE) };
    UiElement {
        id: id.into(),
        parent: Some("chips".into()),
        order,
        width: Size::Fixed(w.ceil()),
        height: Size::Fixed(26.0),
        padding: 6.0,
        background_color: if head { th::SELECTED_BG } else { th::RAISED },
        corner_radius: th::RADIUS_CHIP,
        border_width: 1.0,
        border_color: border,
        text: Some(label.into()),
        text_size: th::TEXT_CAPTION,
        text_color: fg,
        text_mono: true,
        text_align: 0.5,
        center_cross: true,
        ..Default::default()
    }
}
