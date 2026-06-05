mod food;
mod player;

use interstice_sdk::*;

use crate::{
    food::{Food, HasFoodEditHandle, spawn_missing_foods},
    player::{BASE_SPEED, DeadPlayer, HasDeadPlayerEditHandle, HasPlayerEditHandle, Player},
};

interstice_module!(visibility: Public);

const WORLD_SIZE: f32 = 2_000.0;

pub fn rand_pos() -> Vec2 {
    let rx = deterministic_random_u64().unwrap_or(0) as f32 / u64::MAX as f32;
    let ry = deterministic_random_u64().unwrap_or(0) as f32 / u64::MAX as f32;
    Vec2 {
        x: (rx * 2.0 - 1.0) * WORLD_SIZE,
        y: (ry * 2.0 - 1.0) * WORLD_SIZE,
    }
}
const DT_MS: u64 = 8;

#[interstice_type]
#[derive(Debug)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

#[reducer(on = "load")]
pub fn init<Caps>(ctx: ReducerContext<Caps>)
where
    Caps: CanRead<Food>
        + CanInsert<Food>
        + CanUpdate<Food>
        + CanDelete<Food>
        + CanRead<Player>
        + CanInsert<Player>
        + CanUpdate<Player>
        + CanDelete<Player>,
{
    ctx.log("agar-server ready");
    spawn_missing_foods(&ctx);
    ctx.schedule("tick", DT_MS).expect("Couldn't schedule tick");
}

#[reducer]
pub fn tick<Caps>(ctx: ReducerContext<Caps>)
where
    Caps: CanRead<Food>
        + CanInsert<Food>
        + CanUpdate<Food>
        + CanDelete<Food>
        + CanRead<Player>
        + CanInsert<Player>
        + CanUpdate<Player>
        + CanDelete<Player>
        + CanInsert<DeadPlayer>
        + CanRead<DeadPlayer>
        + CanDelete<DeadPlayer>,
{
    if ctx.caller_node_id != ctx.current_node_id() {
        ctx.log("tick can only be called by the server itself");
        return;
    }

    // Clear last tick's death notifications.
    for dead in ctx.current.tables.deadplayer().scan() {
        let _ = ctx.current.tables.deadplayer().delete(dead.id);
    }

    let dt = DT_MS as f32 / 1000.0;
    let mut players = ctx.current.tables.player().scan();

    // Move players.
    for p in players.iter_mut() {
        let speed = BASE_SPEED / (1.0 + p.radius * 0.03);
        p.pos.x = (p.pos.x + p.dir.x * speed * dt).clamp(-WORLD_SIZE, WORLD_SIZE);
        p.pos.y = (p.pos.y + p.dir.y * speed * dt).clamp(-WORLD_SIZE, WORLD_SIZE);
    }

    // Food collisions.
    let foods = ctx.current.tables.food().scan();
    let mut eaten_food_ids = Vec::new();
    for food in &foods {
        for p in players.iter_mut() {
            if collides(&p.pos, p.radius, &food.pos, food.radius) {
                p.radius = grow_radius(p.radius, std::f32::consts::PI * food.radius * food.radius);
                eaten_food_ids.push(food.id);
                break;
            }
        }
    }

    // Player vs player.
    let mut dead_ids: Vec<String> = Vec::new();
    for i in 0..players.len() {
        for j in 0..players.len() {
            if i == j { continue; }
            if players[j].radius > players[i].radius * 1.1
                && collides(&players[i].pos, players[i].radius, &players[j].pos, players[j].radius)
            {
                players[j].radius = grow_radius(
                    players[j].radius,
                    std::f32::consts::PI * players[i].radius * players[i].radius,
                );
                dead_ids.push(players[i].id.clone());
                break;
            }
        }
    }

    // Commit player updates and deaths.
    for p in &players {
        if dead_ids.contains(&p.id) {
            let _ = ctx.current.tables.player().delete(p.id.clone());
            let _ = ctx.current.tables.deadplayer().insert(DeadPlayer { id: p.id.clone() });
        } else {
            let _ = ctx.current.tables.player().update(p.clone());
        }
    }

    for food_id in eaten_food_ids {
        let _ = ctx.current.tables.food().delete(food_id);
    }

    spawn_missing_foods(&ctx);
    ctx.schedule("tick", DT_MS).expect("Couldn't schedule tick");
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
