use crate::{Vec2, rand_pos};
use interstice_sdk::*;

pub const BASE_SPEED: f32 = 240.0;
const START_RADIUS: f32 = 18.0;

#[interstice_type]
#[derive(Debug, Copy)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

const PLAYER_COLORS: [Color; 8] = [
    Color { r: 0.95, g: 0.35, b: 0.35 }, // red
    Color { r: 0.35, g: 0.75, b: 0.95 }, // sky blue
    Color { r: 0.40, g: 0.90, b: 0.50 }, // green
    Color { r: 0.95, g: 0.75, b: 0.25 }, // yellow
    Color { r: 0.80, g: 0.45, b: 0.95 }, // purple
    Color { r: 0.95, g: 0.55, b: 0.20 }, // orange
    Color { r: 0.30, g: 0.85, b: 0.85 }, // cyan
    Color { r: 0.95, g: 0.45, b: 0.75 }, // pink
];

#[table(public, stateful)]
#[derive(Debug)]
pub struct Player {
    #[primary_key]
    pub id: String,
    pub name: String,
    pub pos: Vec2,
    pub dir: Vec2,
    pub radius: f32,
    pub color: Color,
}

/// Stateful notification that a player was eaten this tick.
/// Cleared at the start of each tick.
#[table(public, stateful)]
#[derive(Debug)]
pub struct DeadPlayer {
    #[primary_key]
    pub id: String,
}

#[reducer]
pub fn join<Caps>(ctx: ReducerContext<Caps>, name: String)
where
    Caps: CanRead<Player> + CanInsert<Player>,
{
    if ctx.current.tables.player().get(ctx.caller_node_id.clone()).is_some() {
        return;
    }

    let pos = rand_pos();
    let count = ctx.current.tables.player().scan().len();
    let color = PLAYER_COLORS[count % PLAYER_COLORS.len()];

    let player = Player {
        id: ctx.caller_node_id.clone(),
        name,
        pos,
        dir: Vec2 { x: 0.0, y: 0.0 },
        radius: START_RADIUS,
        color,
    };
    let _ = ctx.current.tables.player().insert(player);
}

#[reducer]
pub fn set_direction<Caps>(ctx: ReducerContext<Caps>, dx: f32, dy: f32)
where
    Caps: CanRead<Player> + CanUpdate<Player>,
{
    let Some(mut p) = ctx.current.tables.player().get(ctx.caller_node_id) else {
        return;
    };
    let len = (dx * dx + dy * dy).sqrt();
    if len > 0.001 {
        p.dir = Vec2 { x: dx / len, y: dy / len };
    } else {
        p.dir = Vec2 { x: 0.0, y: 0.0 };
    }
    let _ = ctx.current.tables.player().update(p);
}
