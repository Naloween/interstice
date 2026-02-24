use interstice_sdk::*;

interstice_module!(visibility: Public);

const WORLD_SIZE: f32 = 2_000.0;
const BASE_SPEED: f32 = 240.0;
const FOOD_RADIUS: f32 = 6.0;
const START_RADIUS: f32 = 18.0;
const TARGET_FOOD: usize = 180;

#[interstice_type]
#[derive(Debug, Clone)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

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

#[table(public, stateful)]
#[derive(Debug, Clone)]
pub struct Food {
    #[primary_key(auto_inc)]
    pub id: u64,
    pub pos: Vec2,
    pub radius: f32,
}

#[interstice_type]
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub players: Vec<Player>,
    pub foods: Vec<Food>,
}

#[reducer(on = "init")]
pub fn init(ctx: ReducerContext) {
    ctx.log("agar-server ready");
    ensure_food(&ctx, TARGET_FOOD);
}

#[reducer]
pub fn join(ctx: ReducerContext, id: String, name: String) {
    if ctx.current.tables.player().get(id.clone()).is_some() {
        return;
    }

    let pos = rand_pos(&ctx);
    let player = Player {
        id: id.clone(),
        name,
        pos,
        dir: Vec2 { x: 0.0, y: 0.0 },
        radius: START_RADIUS,
    };
    let _ = ctx.current.tables.player().insert(player);
    ensure_food(&ctx, TARGET_FOOD);
}

#[reducer]
pub fn set_direction(ctx: ReducerContext, id: String, dx: f32, dy: f32) {
    let Some(mut p) = ctx.current.tables.player().get(id) else {
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

#[reducer]
pub fn tick(ctx: ReducerContext, dt_ms: Option<u64>) {
    let dt = dt_ms.unwrap_or(16) as f32 / 1000.0;

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

    ensure_food(&ctx, TARGET_FOOD);
}

#[query]
pub fn snapshot(ctx: QueryContext) -> Snapshot {
    let players = ctx.current.tables.player().scan().unwrap_or_default();
    let foods = ctx.current.tables.food().scan().unwrap_or_default();
    Snapshot { players, foods }
}

fn rand_pos(ctx: &ReducerContext) -> Vec2 {
    let rx = deterministic_random_u64().unwrap_or(0) as f32 / u64::MAX as f32;
    let ry = deterministic_random_u64().unwrap_or(0) as f32 / u64::MAX as f32;
    Vec2 {
        x: (rx * 2.0 - 1.0) * WORLD_SIZE,
        y: (ry * 2.0 - 1.0) * WORLD_SIZE,
    }
}

fn ensure_food(ctx: &ReducerContext, target: usize) {
    let mut count = ctx.current.tables.food().scan().unwrap_or_default().len();
    while count < target {
        let pos = rand_pos(ctx);
        let _ = ctx.current.tables.food().insert(Food {
            id: 0,
            pos,
            radius: FOOD_RADIUS,
        });
        count += 1;
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
