use crate::{Vec2, rand_pos};
use crate::player::Color;
use interstice_sdk::*;

const FOOD_RADIUS: f32 = 6.0;
const TARGET_FOOD: usize = 180;

const FOOD_COLORS: [Color; 6] = [
    Color { r: 0.95, g: 0.35, b: 0.35 },
    Color { r: 0.35, g: 0.90, b: 0.50 },
    Color { r: 0.35, g: 0.70, b: 0.95 },
    Color { r: 0.95, g: 0.80, b: 0.25 },
    Color { r: 0.85, g: 0.45, b: 0.95 },
    Color { r: 0.95, g: 0.55, b: 0.20 },
];

#[table(public, stateful)]
#[derive(Debug)]
pub struct Food {
    #[primary_key(auto_inc)]
    pub id: u64,
    pub pos: Vec2,
    pub radius: f32,
    pub color: Color,
}

pub fn spawn_missing_foods<Caps>(ctx: &ReducerContext<Caps>)
where
    Caps: CanRead<Food> + CanInsert<Food>,
{
    let mut count = ctx.current.tables.food().scan().len();
    while count < TARGET_FOOD {
        let pos = rand_pos();
        let color_idx = (deterministic_random_u64().unwrap_or(count as u64) as usize) % FOOD_COLORS.len();
        let color = FOOD_COLORS[color_idx];
        let _ = ctx.current.tables.food().insert(Food {
            id: 0,
            pos,
            radius: FOOD_RADIUS,
            color,
        });
        count += 1;
    }
}
