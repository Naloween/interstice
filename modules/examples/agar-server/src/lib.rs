mod food;
mod player;
mod snapshot;

use interstice_sdk::*;

use crate::{
    food::{HasFoodEditHandle, spawn_missing_foods},
    player::{BASE_SPEED, HasPlayerEditHandle},
};

interstice_module!(visibility: Public);

const WORLD_SIZE: f32 = 2_000.0;

const DT_MS: u64 = 50;

#[interstice_type]
#[derive(Debug, Clone)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

#[reducer(on = "load")]
pub fn init(ctx: ReducerContext) {
    ctx.log("agar-server ready");
    spawn_missing_foods(&ctx);
    ctx.schedule("tick", DT_MS).expect("Couldn't schedule tick");
}

#[reducer]
pub fn tick(ctx: ReducerContext) {
    if ctx.caller_node_id != ctx.current_node_id() {
        ctx.log("tick can only be called by the server itself");
        return;
    }
    let dt = DT_MS as f32 / 1000.0;
    let mut players = ctx.current.tables.player().scan().unwrap_or_default();

    // Move players
    for p in players.iter_mut() {
        let speed = BASE_SPEED / (1.0 + p.radius * 0.03);
        p.pos.x += p.dir.x * speed * dt;
        p.pos.y += p.dir.y * speed * dt;
        p.pos.x = p.pos.x.clamp(-WORLD_SIZE, WORLD_SIZE);
        p.pos.y = p.pos.y.clamp(-WORLD_SIZE, WORLD_SIZE);
    }

    // Food collisions
    let mut foods = ctx.current.tables.food().scan().unwrap_or_default();
    foods.retain(|f| {
        let mut eaten = false;
        for p in players.iter_mut() {
            if collides(&p.pos, p.radius, &f.pos, f.radius) {
                let added_area = std::f32::consts::PI * f.radius * f.radius;
                p.radius = grow_radius(p.radius, added_area);
                eaten = true;
                break;
            }
        }
        !eaten
    });

    // Player vs player
    let mut alive = Vec::new();
    for i in 0..players.len() {
        let mut dead = false;
        for j in 0..players.len() {
            if i == j {
                continue;
            }
            let big = players[j].radius;
            let small = players[i].radius;
            if big > small * 1.1
                && collides(
                    &players[i].pos,
                    players[i].radius,
                    &players[j].pos,
                    players[j].radius,
                )
            {
                let gain_area = std::f32::consts::PI * players[i].radius * players[i].radius;
                players[j].radius = grow_radius(players[j].radius, gain_area);
                dead = true;
                break;
            }
        }
        if !dead {
            alive.push(players[i].clone());
        }
    }

    // Persist players
    for row in alive.into_iter() {
        let _ = ctx.current.tables.player().update(row.clone());
    }

    // Persist foods (rewrite table)
    let _ = ctx.current.tables.food().clear();
    for f in foods {
        let _ = ctx.current.tables.food().insert(f);
    }

    spawn_missing_foods(&ctx);

    ctx.schedule("tick", DT_MS).expect("Couldn't schedule tick");
}

fn rand_pos() -> Vec2 {
    let rx = deterministic_random_u64().unwrap_or(0) as f32 / u64::MAX as f32;
    let ry = deterministic_random_u64().unwrap_or(0) as f32 / u64::MAX as f32;
    Vec2 {
        x: (rx * 2.0 - 1.0) * WORLD_SIZE,
        y: (ry * 2.0 - 1.0) * WORLD_SIZE,
    }
}

fn collides(a: &Vec2, ar: f32, b: &Vec2, br: f32) -> bool {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    let r = ar + br;
    dx * dx + dy * dy <= r * r
}

fn grow_radius(current: f32, added_area: f32) -> f32 {
    let area = std::f32::consts::PI * current * current + added_area;
    (area / std::f32::consts::PI).sqrt()
}
