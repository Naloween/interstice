use interstice_sdk::*;

#[interstice_type]
#[derive(Debug, PartialEq)]
pub enum GameState {
    Lobby,
    InGame,
    Dead,
}

#[table(ephemeral)]
pub struct ClientState {
    #[primary_key]
    pub id: u32,
    pub state: GameState,
    pub zoom: f32,
    pub target_zoom: f32,
    pub final_score: f32,
}
