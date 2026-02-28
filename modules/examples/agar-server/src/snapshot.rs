use crate::{
    food::{Food, HasFoodReadHandle},
    player::{HasPlayerReadHandle, Player},
};
use interstice_sdk::*;

#[interstice_type]
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub players: Vec<Player>,
    pub foods: Vec<Food>,
}

#[query]
pub fn snapshot(ctx: QueryContext) -> Snapshot {
    let players = ctx.current.tables.player().scan().unwrap_or_default();
    let foods = ctx.current.tables.food().scan().unwrap_or_default();
    Snapshot { players, foods }
}
