use crate::bindings::{
    agar_server_example::{agar_server::*, *},
    *,
};
use crate::ui::{self, LayoutDirection, Size, TextWrap, UiElement};
use interstice_sdk::*;

const UI_HUD_SCORE: &str = "hud_score";
const UI_HUD_LB_ROOT: &str = "hud_lb_root";
const UI_HUD_LB_TITLE: &str = "hud_lb_title";

pub fn build_hud_ui<Caps>(ctx: &ReducerContext<Caps>)
where
    Caps: CanInsert<UiElement> + CanUpdate<UiElement>,
{
    let none = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let text_col = (0.92f32, 0.92f32, 0.95f32, 1.0f32);
    let panel_bg = (0.07f32, 0.07f32, 0.10f32, 0.80f32);

    // Single root row spanning the full width at the top.
    ui::create_element(ctx, UiElement {
        id: "hud_root".into(),
        parent: None,
        order: 10,
        width: Size::Grow,
        height: Size::Fit,
        layout_direction: LayoutDirection::Row,
        gap: 0.0,
        padding: 12.0,
        margin: 0.0,
        background_color: none,
        corner_radius: 0.0,
        border_width: 0.0,
        border_color: none,
        text: None,
        text_size: 0.0,
        text_color: none,
        text_wrap: TextWrap::None,
        image: None,
        is_input: false,
        cursor_pos: 0,
        scrollable_x: false,
        scrollable_y: false,
        scroll_x: 0.0,
        scroll_y: 0.0,
        visible: true,
        ..Default::default()
    });

    // Score label (top-left).
    ui::create_element(ctx, UiElement {
        id: UI_HUD_SCORE.into(),
        parent: Some("hud_root".into()),
        order: 0,
        width: Size::Fit,
        height: Size::Fit,
        layout_direction: LayoutDirection::Row,
        gap: 0.0,
        padding: 8.0,
        margin: 0.0,
        background_color: panel_bg,
        corner_radius: 6.0,
        border_width: 0.0,
        border_color: none,
        text: Some("Score: 0".into()),
        text_size: 14.0,
        text_color: text_col,
        text_wrap: TextWrap::None,
        image: None,
        is_input: false,
        cursor_pos: 0,
        scrollable_x: false,
        scrollable_y: false,
        scroll_x: 0.0,
        scroll_y: 0.0,
        visible: true,
        ..Default::default()
    });

    // Middle spacer — pushes leaderboard to the right.
    ui::create_element(ctx, UiElement {
        id: "hud_mid_spacer".into(),
        parent: Some("hud_root".into()),
        order: 1,
        width: Size::Grow,
        height: Size::Grow,
        layout_direction: LayoutDirection::Row,
        gap: 0.0,
        padding: 0.0,
        margin: 0.0,
        background_color: none,
        corner_radius: 0.0,
        border_width: 0.0,
        border_color: none,
        text: None,
        text_size: 0.0,
        text_color: none,
        text_wrap: TextWrap::None,
        image: None,
        is_input: false,
        cursor_pos: 0,
        scrollable_x: false,
        scrollable_y: false,
        scroll_x: 0.0,
        scroll_y: 0.0,
        visible: true,
        ..Default::default()
    });

    // Leaderboard panel (top-right).
    ui::create_element(ctx, UiElement {
        id: UI_HUD_LB_ROOT.into(),
        parent: Some("hud_root".into()),
        order: 2,
        width: Size::Fit,
        height: Size::Fit,
        layout_direction: LayoutDirection::Column,
        gap: 4.0,
        padding: 10.0,
        margin: 0.0,
        background_color: panel_bg,
        corner_radius: 8.0,
        border_width: 0.0,
        border_color: none,
        text: None,
        text_size: 0.0,
        text_color: none,
        text_wrap: TextWrap::None,
        image: None,
        is_input: false,
        cursor_pos: 0,
        scrollable_x: false,
        scrollable_y: false,
        scroll_x: 0.0,
        scroll_y: 0.0,
        visible: true,
        ..Default::default()
    });
    ui::create_element(ctx, UiElement {
        id: UI_HUD_LB_TITLE.into(),
        parent: Some(UI_HUD_LB_ROOT.into()),
        order: 0,
        width: Size::Fit,
        height: Size::Fit,
        layout_direction: LayoutDirection::Row,
        gap: 0.0,
        padding: 0.0,
        margin: 0.0,
        background_color: none,
        corner_radius: 0.0,
        border_width: 0.0,
        border_color: none,
        text: Some("Leaderboard".into()),
        text_size: 13.0,
        text_color: (0.7, 0.7, 0.8, 1.0),
        text_wrap: TextWrap::None,
        image: None,
        is_input: false,
        cursor_pos: 0,
        scrollable_x: false,
        scrollable_y: false,
        scroll_x: 0.0,
        scroll_y: 0.0,
        visible: true,
        ..Default::default()
    });

    // Pre-create 8 leaderboard row slots, initially hidden. update_hud only
    // toggles `visible`/`text` on these — never delete+recreate in one run
    // (which would trip the unique-constraint check against the committed row).
    for i in 0..8 {
        ui::create_element(ctx, UiElement {
            id: format!("hud_lb_{i}"),
            parent: Some(UI_HUD_LB_ROOT.into()),
            order: (i + 1) as u32,
            width: Size::Grow,
            height: Size::Fit,
            layout_direction: LayoutDirection::Row,
            gap: 0.0,
            padding: 2.0,
            margin: 0.0,
            background_color: none,
            corner_radius: 0.0,
            border_width: 0.0,
            border_color: none,
            text: Some(String::new()),
            text_size: 11.0,
            text_color: text_col,
            text_wrap: TextWrap::None,
            image: None,
            is_input: false,
            cursor_pos: 0,
            scrollable_x: false,
            scrollable_y: false,
            scroll_x: 0.0,
            scroll_y: 0.0,
            visible: false,
            ..Default::default()
        });
    }
}

pub fn update_hud<Caps>(ctx: &ReducerContext<Caps>, _sw: f32, _sh: f32)
where
    Caps: CanRead<Player> + CanUpdate<UiElement>,
{
    let my_id = ctx.current_node_id();
    let mut players: Vec<Player> = ctx
        .agar_server_example()
        .agar_server()
        .tables
        .player()
        .scan();
    players.sort_by(|a, b| {
        b.radius
            .partial_cmp(&a.radius)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let none = (0.0f32, 0.0f32, 0.0f32, 0.0f32);

    // Update score label.
    if let Some(my_player) = players.iter().find(|p| p.id == my_id) {
        ui::update_element(ctx, UiElement {
            id: UI_HUD_SCORE.into(),
            parent: Some("hud_root".into()),
            order: 0,
            width: Size::Fit,
            height: Size::Fit,
            layout_direction: LayoutDirection::Row,
            gap: 0.0,
            padding: 8.0,
            margin: 0.0,
            background_color: (0.07, 0.07, 0.10, 0.80),
            corner_radius: 6.0,
            border_width: 0.0,
            border_color: none,
            text: Some(format!("Score: {}", my_player.radius as u32)),
            text_size: 14.0,
            text_color: (0.92, 0.92, 0.95, 1.0),
            text_wrap: TextWrap::None,
            image: None,
            is_input: false,
            cursor_pos: 0,
            scrollable_x: false,
            scrollable_y: false,
            scroll_x: 0.0,
            scroll_y: 0.0,
            visible: true,
            ..Default::default()
        });
    }

    // Update the 8 pre-created leaderboard slots in place: fill + show the rows
    // backed by a player, hide the rest. Never delete/recreate within one run.
    let text_col = (0.92f32, 0.92f32, 0.95f32, 1.0f32);
    for i in 0..8 {
        let row = players.get(i).map(|p| {
            let is_me = p.id == my_id;
            let name_color = if is_me {
                (p.color.r, p.color.g, p.color.b, 1.0)
            } else {
                text_col
            };
            let label = format!("{}. {} ({})", i + 1, p.name, p.radius as u32);
            (label, name_color)
        });
        let (text, color, visible) = match row {
            Some((label, color)) => (label, color, true),
            None => (String::new(), text_col, false),
        };
        ui::update_element(ctx, UiElement {
            id: format!("hud_lb_{i}"),
            parent: Some(UI_HUD_LB_ROOT.into()),
            order: (i + 1) as u32,
            width: Size::Grow,
            height: Size::Fit,
            layout_direction: LayoutDirection::Row,
            gap: 0.0,
            padding: 2.0,
            margin: 0.0,
            background_color: none,
            corner_radius: 0.0,
            border_width: 0.0,
            border_color: none,
            text: Some(text),
            text_size: 11.0,
            text_color: color,
            text_wrap: TextWrap::None,
            image: None,
            is_input: false,
            cursor_pos: 0,
            scrollable_x: false,
            scrollable_y: false,
            scroll_x: 0.0,
            scroll_y: 0.0,
            visible,
            ..Default::default()
        });
    }
}
