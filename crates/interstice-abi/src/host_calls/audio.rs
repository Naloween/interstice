use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub enum AudioCall {
    Noop,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum AudioResponse {
    Ok,
    Err(String),
}
