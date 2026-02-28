use crate::{Vec2, rand_pos};
use interstice_sdk::*;

const FOOD_RADIUS: f32 = 6.0;
const TARGET_FOOD: usize = 180;

#[table(public, stateful)]
#[derive(Debug, Clone)]
pub struct Food {
    #[primary_key(auto_inc)]
    pub id: u64,
    pub pos: Vec2,
    pub radius: f32,
}

pub fn spawn_missing_foods(ctx: &ReducerContext) {
    let mut count = ctx.current.tables.food().scan().unwrap_or_default().len();
    while count < TARGET_FOOD {
        let pos = rand_pos();
        let _ = ctx.current.tables.food().insert(Food {
            id: 0,
            pos,
            radius: FOOD_RADIUS,
        });
        count += 1;
    }
}
