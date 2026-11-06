use interstice_sdk::*;

#[reducer]
pub fn small(_ctx: ReducerContext, n: u16) {}

#[reducer]
pub fn borrowed(_ctx: ReducerContext, s: &str) {}

#[interstice_type]
pub struct Map {
    pub entries: std::collections::HashMap<String, u32>,
}

fn main() {}
