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
    /// Camera centre in world units, eased towards our cell each frame.
    pub cam_x: f32,
    pub cam_y: f32,
    /// Left button last frame, for click edges.
    pub was_down: bool,
    /// Signature of what the HUD shows; it is rewritten only on change.
    pub hud_sig: u64,
    /// A name asked for by the `play` reducer (scripting), joined next frame.
    pub pending_join: Option<String>,
}
