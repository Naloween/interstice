use crate::{Vec2, rand_pos};
use interstice_sdk::*;

pub const BASE_SPEED: f32 = 240.0;
const START_RADIUS: f32 = 18.0;

#[table(public, stateful)]
#[derive(Debug, Clone)]
pub struct Player {
    #[primary_key]
    pub id: String,
    pub name: String,
    pub pos: Vec2,
    pub dir: Vec2,
    pub radius: f32,
}

#[reducer]
pub fn join(ctx: ReducerContext, name: String) {
    if ctx
        .current
        .tables
        .player()
        .get(ctx.caller_node_id.clone())
        .is_some()
    {
        return;
    }

    let pos = rand_pos();
    let player = Player {
        id: ctx.caller_node_id.clone(),
        name,
        pos,
        dir: Vec2 { x: 0.0, y: 0.0 },
        radius: START_RADIUS,
    };
    let _ = ctx.current.tables.player().insert(player);
}

#[reducer]
pub fn set_direction(ctx: ReducerContext, dx: f32, dy: f32) {
    let Some(mut p) = ctx.current.tables.player().get(ctx.caller_node_id) else {
        return;
    };
    let len = (dx * dx + dy * dy).sqrt();
    if len > 0.001 {
        p.dir = Vec2 {
            x: dx / len,
            y: dy / len,
        };
    } else {
        p.dir = Vec2 { x: 0.0, y: 0.0 };
    }
    let _ = ctx.current.tables.player().update(p);
}
