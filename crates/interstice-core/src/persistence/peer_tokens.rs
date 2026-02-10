use crate::error::IntersticeError;
use crate::node::NodeId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PeerTokenFile {
    local_token: String,
    peers: HashMap<String, String>,
}

pub struct PeerTokenStore {
    path: Option<PathBuf>,
    local_token: String,
    peers: HashMap<String, String>,
}

impl PeerTokenStore {
    pub fn load_or_create<P: AsRef<Path>>(path: P) -> Result<Self, IntersticeError> {
        let path = path.as_ref().to_path_buf();
        if path.exists() {
            let contents = std::fs::read_to_string(&path).map_err(|err| {
                IntersticeError::Internal(format!(
                    "Failed to read peer token store {}: {err}",
                    path.display()
                ))
            })?;
            let file: PeerTokenFile = if contents.trim().is_empty() {
                PeerTokenFile {
                    local_token: Self::generate_token(),
                    peers: HashMap::new(),
                }
            } else {
                toml::from_str(&contents).map_err(|err| {
                    IntersticeError::Internal(format!(
                        "Failed to parse peer token store {}: {err}",
                        path.display()
                    ))
                })?
            };
            let loaded_local_token = file.local_token.clone();
            let mut store = Self {
                path: Some(path),
                local_token: file.local_token,
                peers: file.peers,
            };
            if store.local_token.trim().is_empty() {
                store.local_token = Self::generate_token();
            }
            if contents.trim().is_empty() || store.local_token != loaded_local_token {
                store.save()?;
            }
            Ok(store)
        } else {
            let store = Self {
                path: Some(path),
                local_token: Self::generate_token(),
                peers: HashMap::new(),
            };
            store.save()?;
            Ok(store)
        }
    }

    pub fn new_in_memory() -> Self {
        Self {
            path: None,
            local_token: Self::generate_token(),
            peers: HashMap::new(),
        }
    }

    pub fn local_token(&self) -> String {
        self.local_token.clone()
    }

    pub fn get_peer_token(&self, peer_id: &NodeId) -> Option<String> {
        self.peers.get(&peer_id.to_string()).cloned()
    }

    pub fn set_peer_token(
        &mut self,
        peer_id: &NodeId,
        token: String,
    ) -> Result<(), IntersticeError> {
        self.peers.insert(peer_id.to_string(), token);
        self.save()
    }

    pub fn save(&self) -> Result<(), IntersticeError> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let file = PeerTokenFile {
            local_token: self.local_token.clone(),
            peers: self.peers.clone(),
        };
        let contents = toml::to_string_pretty(&file).map_err(|err| {
            IntersticeError::Internal(format!("Failed to serialize peer token store: {err}"))
        })?;
        std::fs::write(path, contents).map_err(|err| {
            IntersticeError::Internal(format!(
                "Failed to write peer token store {}: {err}",
                path.display()
            ))
        })?;
        Ok(())
    }

    fn generate_token() -> String {
        Uuid::new_v4().to_string()
    }
}
