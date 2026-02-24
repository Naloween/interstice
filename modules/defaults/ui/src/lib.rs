use interstice_sdk::*;

interstice_module!(visibility: Private);

use crate::bindings::{
    graphics::{Color as GraphicsColor, Rect as GraphicsRect, Vec2 as GraphicsVec2, *},
    *,
};

const DEFAULT_SCOPE: &str = "default";
const DEFAULT_LAYER: &str = "ui.default";

#[interstice_type]
#[derive(Debug, Clone)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

#[interstice_type]
#[derive(Debug, Clone)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

#[interstice_type]
#[derive(Debug, Clone)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[table(stateful)]
#[derive(Debug, Clone)]
pub struct UiLayerConfig {
    #[primary_key]
    pub scope: String,
    pub layer_name: String,
    pub z: i32,
    pub clear: bool,
    pub enabled: bool,
}

#[table(stateful)]
#[derive(Debug, Clone)]
pub struct UiLabel {
    #[primary_key]
    pub key: (String, String),
    pub text: String,
    pub position: Vec2,
    pub size: f32,
    pub color: Color,
    pub font: Option<String>,
    pub visible: bool,
}

#[table(stateful)]
#[derive(Debug, Clone)]
pub struct UiPanel {
    #[primary_key]
    pub key: (String, String),
    pub rect: Rect,
    pub background: Color,
    pub border_color: Option<Color>,
    pub border_width: f32,
    pub title: Option<String>,
    pub title_color: Color,
    pub title_size: f32,
    pub padding: f32,
    pub visible: bool,
}

#[reducer(on = "load")]
pub fn load(ctx: ReducerContext) {
    if ctx
        .current
        .tables
        .uilayerconfig()
        .get(DEFAULT_SCOPE.to_string())
        .is_none()
    {
        let _ = ctx.current.tables.uilayerconfig().insert(UiLayerConfig {
            scope: DEFAULT_SCOPE.to_string(),
            layer_name: DEFAULT_LAYER.to_string(),
            z: 100,
            clear: false,
            enabled: true,
        });
    }

    if ctx
        .current
        .tables
        .uipanel()
        .get((DEFAULT_SCOPE.to_string(), "demo".to_string()))
        .is_none()
    {
        let _ = ctx.current.tables.uipanel().insert(UiPanel {
            key: (DEFAULT_SCOPE.to_string(), "demo".to_string()),
            rect: Rect {
                x: 40.0,
                y: 40.0,
                w: 360.0,
                h: 160.0,
            },
            background: Color {
                r: 0.08,
                g: 0.12,
                b: 0.20,
                a: 0.94,
            },
            border_color: Some(Color {
                r: 0.35,
                g: 0.65,
                b: 0.95,
                a: 1.0,
            }),
            border_width: 2.0,
            title: Some("UI Demo".to_string()),
            title_color: Color {
                r: 0.9,
                g: 0.95,
                b: 1.0,
                a: 1.0,
            },
            title_size: 18.0,
            padding: 10.0,
            visible: true,
        });
    }

    if ctx
        .current
        .tables
        .uilabel()
        .get((DEFAULT_SCOPE.to_string(), "demo_label".to_string()))
        .is_none()
    {
        let _ = ctx.current.tables.uilabel().insert(UiLabel {
            key: (DEFAULT_SCOPE.to_string(), "demo_label".to_string()),
            text: "Hello from UI via graphics.frametick".to_string(),
            position: Vec2 { x: 60.0, y: 90.0 },
            size: 16.0,
            color: Color {
                r: 0.95,
                g: 0.95,
                b: 0.98,
                a: 1.0,
            },
            font: None,
            visible: true,
        });
    }
}

#[reducer]
pub fn configure_scope(
    ctx: ReducerContext,
    scope: String,
    layer_name: String,
    z: i32,
    clear: bool,
    enabled: bool,
) {
    if scope.trim().is_empty() {
        ctx.log("Scope cannot be empty");
        return;
    }
    if layer_name.trim().is_empty() {
        ctx.log("Layer name cannot be empty");
        return;
    }

    let row = UiLayerConfig {
        scope: scope.clone(),
        layer_name,
        z,
        clear,
        enabled,
    };

    if ctx.current.tables.uilayerconfig().get(scope).is_some() {
        let _ = ctx.current.tables.uilayerconfig().update(row);
    } else {
        let _ = ctx.current.tables.uilayerconfig().insert(row);
    }
}

#[reducer]
pub fn set_scope_enabled(ctx: ReducerContext, scope: String, enabled: bool) {
    let Some(mut row) = ctx.current.tables.uilayerconfig().get(scope.clone()) else {
        ctx.log(&format!("UI scope '{}' not found", scope));
        return;
    };

    row.enabled = enabled;
    let _ = ctx.current.tables.uilayerconfig().update(row);
}

#[reducer]
pub fn upsert_label(
    ctx: ReducerContext,
    scope: String,
    id: String,
    text: String,
    position: Vec2,
    size: f32,
    color: Color,
    font: Option<String>,
    visible: bool,
) {
    if scope.trim().is_empty() || id.trim().is_empty() {
        ctx.log("Scope and id are required for upsert_label");
        return;
    }
    if size <= 0.0 {
        ctx.log("Label size must be positive");
        return;
    }

    let key = (scope, id);
    let row = UiLabel {
        key: key.clone(),
        text,
        position,
        size,
        color,
        font,
        visible,
    };

    if ctx.current.tables.uilabel().get(key).is_some() {
        let _ = ctx.current.tables.uilabel().update(row);
    } else {
        let _ = ctx.current.tables.uilabel().insert(row);
    }
}

#[reducer]
pub fn remove_label(ctx: ReducerContext, scope: String, id: String) {
    if id.trim().is_empty() {
        return;
    }
    let _ = ctx.current.tables.uilabel().delete((scope, id));
}

#[reducer]
pub fn upsert_panel(
    ctx: ReducerContext,
    scope: String,
    id: String,
    rect: Rect,
    background: Color,
    border_color: Option<Color>,
    border_width: f32,
    title: Option<String>,
    title_color: Color,
    title_size: f32,
    padding: f32,
    visible: bool,
) {
    if scope.trim().is_empty() || id.trim().is_empty() {
        ctx.log("Scope and id are required for upsert_panel");
        return;
    }
    if rect.w <= 0.0 || rect.h <= 0.0 {
        ctx.log("Panel width and height must be positive");
        return;
    }

    let key = (scope, id);
    let row = UiPanel {
        key: key.clone(),
        rect,
        background,
        border_color,
        border_width: border_width.max(0.0001),
        title,
        title_color,
        title_size: if title_size > 0.0 { title_size } else { 14.0 },
        padding: padding.max(0.0),
        visible,
    };

    if ctx.current.tables.uipanel().get(key).is_some() {
        let _ = ctx.current.tables.uipanel().update(row);
    } else {
        let _ = ctx.current.tables.uipanel().insert(row);
    }
}

#[reducer]
pub fn remove_panel(ctx: ReducerContext, scope: String, id: String) {
    if id.trim().is_empty() {
        return;
    }
    let _ = ctx.current.tables.uipanel().delete((scope, id));
}

#[reducer]
pub fn clear_scope(ctx: ReducerContext, scope: String) {
    if let Ok(rows) = ctx.current.tables.uilabel().scan() {
        for row in rows {
            if row.key.0 == scope {
                let _ = ctx.current.tables.uilabel().delete(row.key);
            }
        }
    }

    if let Ok(rows) = ctx.current.tables.uipanel().scan() {
        for row in rows {
            if row.key.0 == scope {
                let _ = ctx.current.tables.uipanel().delete(row.key);
            }
        }
    }
}

#[reducer(on = "graphics.frametick.update")]
pub fn render(ctx: ReducerContext, _prev: FrameTick, tick: FrameTick) {
    ctx.log(&format!("ui render tick {}", tick.frame));
    let layer_rows = ctx
        .current
        .tables
        .uilayerconfig()
        .scan()
        .unwrap_or_default();
    let graphics = ctx.graphics();

    for layer in layer_rows.into_iter().filter(|item| item.enabled) {
        ensure_graphics_layer(&ctx, &graphics.reducers, &layer);
        render_panels_for_scope(&ctx, &graphics.reducers, &layer.scope, &layer.layer_name);
        render_labels_for_scope(&ctx, &graphics.reducers, &layer.scope, &layer.layer_name);
    }
}

fn ensure_graphics_layer(ctx: &ReducerContext, graphics: &GraphicsReducers, layer: &UiLayerConfig) {
    if let Err(err) = graphics.create_layer(layer.layer_name.clone(), layer.z, layer.clear) {
        ctx.log(&format!("UI layer create failed: {err}"));
    }
    if let Err(err) = graphics.set_layer_z(layer.layer_name.clone(), layer.z) {
        ctx.log(&format!("UI layer set z failed: {err}"));
    }
    if let Err(err) = graphics.set_layer_clear(layer.layer_name.clone(), layer.clear) {
        ctx.log(&format!("UI layer set clear failed: {err}"));
    }
}

fn render_panels_for_scope(
    ctx: &ReducerContext,
    graphics: &GraphicsReducers,
    scope: &str,
    layer_name: &str,
) {
    let rows = ctx.current.tables.uipanel().scan().unwrap_or_default();
    for panel in rows
        .into_iter()
        .filter(|item| item.key.0 == scope && item.visible)
    {
        let rect = panel.rect.clone();

        if let Err(err) = graphics.draw_rect(
            layer_name.to_string(),
            to_graphics_rect(&rect),
            to_graphics_color(&panel.background),
            true,
            panel.border_width,
        ) {
            ctx.log(&format!("UI draw_rect failed: {err}"));
        }

        if let Some(border_color) = panel.border_color.clone() {
            if let Err(err) = graphics.draw_rect(
                layer_name.to_string(),
                to_graphics_rect(&rect),
                to_graphics_color(&border_color),
                false,
                panel.border_width,
            ) {
                ctx.log(&format!("UI panel border draw failed: {err}"));
            }
        }

        if let Some(title) = panel.title.clone() {
            if !title.is_empty() {
                let text_position = Vec2 {
                    x: rect.x + panel.padding,
                    y: rect.y + panel.padding,
                };

                if let Err(err) = graphics.draw_text(
                    layer_name.to_string(),
                    title,
                    to_graphics_vec2(&text_position),
                    panel.title_size,
                    to_graphics_color(&panel.title_color),
                    None,
                ) {
                    ctx.log(&format!("UI title draw failed: {err}"));
                }
            }
        }
    }
}

fn render_labels_for_scope(
    ctx: &ReducerContext,
    graphics: &GraphicsReducers,
    scope: &str,
    layer_name: &str,
) {
    let rows = ctx.current.tables.uilabel().scan().unwrap_or_default();
    for label in rows
        .into_iter()
        .filter(|item| item.key.0 == scope && item.visible)
    {
        if let Err(err) = graphics.draw_text(
            layer_name.to_string(),
            label.text,
            to_graphics_vec2(&label.position),
            label.size,
            to_graphics_color(&label.color),
            label.font,
        ) {
            ctx.log(&format!("UI label draw failed: {err}"));
        }
    }
}

fn to_graphics_vec2(value: &Vec2) -> GraphicsVec2 {
    GraphicsVec2 {
        x: value.x,
        y: value.y,
    }
}

fn to_graphics_color(value: &Color) -> GraphicsColor {
    GraphicsColor {
        r: value.r,
        g: value.g,
        b: value.b,
        a: value.a,
    }
}

fn to_graphics_rect(value: &Rect) -> GraphicsRect {
    GraphicsRect {
        x: value.x,
        y: value.y,
        w: value.w,
        h: value.h,
    }
}
