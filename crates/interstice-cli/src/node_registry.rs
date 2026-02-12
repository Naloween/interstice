use crate::data_directory::data_file;
use interstice_core::IntersticeError;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRecord {
    pub name: String,
    pub address: String,
    pub node_id: Option<String>,
    pub local: bool,
    pub last_seen: Option<u64>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct NodeRegistry {
    pub nodes: Vec<NodeRecord>,
}

impl NodeRegistry {
    pub fn load() -> Result<Self, IntersticeError> {
        let path = registry_path();
        if path.exists() {
            let contents = fs::read_to_string(&path).map_err(|err| {
                IntersticeError::Internal(format!(
                    "Failed to read node registry {}: {err}",
                    path.display()
                ))
            })?;
            if contents.trim().is_empty() {
                return Ok(Self::default());
            }
            toml::from_str(&contents).map_err(|err| {
                IntersticeError::Internal(format!(
                    "Failed to parse node registry {}: {err}",
                    path.display()
                ))
            })
        } else {
            Ok(Self::default())
        }
    }

    pub fn save(&self) -> Result<(), IntersticeError> {
        let path = registry_path();
        let contents = toml::to_string_pretty(self).map_err(|err| {
            IntersticeError::Internal(format!("Failed to serialize node registry: {err}"))
        })?;
        fs::write(&path, contents).map_err(|err| {
            IntersticeError::Internal(format!(
                "Failed to write node registry {}: {err}",
                path.display()
            ))
        })?;
        Ok(())
    }

    pub fn add(&mut self, record: NodeRecord) -> Result<(), IntersticeError> {
        if self.nodes.iter().any(|n| n.name == record.name) {
            return Err(IntersticeError::Internal(format!(
                "Node name '{}' already exists",
                record.name
            )));
        }
        self.nodes.push(record);
        self.save()
    }

    pub fn remove(&mut self, name_or_id: &str) -> Result<NodeRecord, IntersticeError> {
        let index = self
            .nodes
            .iter()
            .position(|n| n.name == name_or_id || n.node_id.as_deref() == Some(name_or_id))
            .ok_or_else(|| IntersticeError::Internal(format!("Unknown node '{}'", name_or_id)))?;
        let removed = self.nodes.remove(index);
        self.save()?;
        Ok(removed)
    }

    pub fn rename(&mut self, old: &str, new: &str) -> Result<(), IntersticeError> {
        if self.nodes.iter().any(|n| n.name == new) {
            return Err(IntersticeError::Internal(format!(
                "Node name '{}' already exists",
                new
            )));
        }
        let node = self
            .nodes
            .iter_mut()
            .find(|n| n.name == old || n.node_id.as_deref() == Some(old))
            .ok_or_else(|| IntersticeError::Internal(format!("Unknown node '{}'", old)))?;
        node.name = new.to_string();
        self.save()
    }

    pub fn get(&self, name_or_id: &str) -> Option<&NodeRecord> {
        self.nodes
            .iter()
            .find(|n| n.name == name_or_id || n.node_id.as_deref() == Some(name_or_id))
    }

    pub fn get_mut(&mut self, name_or_id: &str) -> Option<&mut NodeRecord> {
        self.nodes
            .iter_mut()
            .find(|n| n.name == name_or_id || n.node_id.as_deref() == Some(name_or_id))
    }

    pub fn resolve_address(&self, name_or_address: &str) -> Option<String> {
        if name_or_address.contains(':') {
            return Some(name_or_address.to_string());
        }
        self.get(name_or_address).map(|n| n.address.clone())
    }

    pub fn list_sorted(&self) -> Vec<NodeRecord> {
        let mut nodes = self.nodes.clone();
        nodes.sort_by(|a, b| a.name.cmp(&b.name));
        nodes
    }

    pub fn set_last_seen(&mut self, name_or_id: &str) {
        if let Some(node) = self.get_mut(name_or_id) {
            node.last_seen = Some(now_epoch());
        }
    }

    pub fn set_node_id(&mut self, name_or_id: &str, node_id: String) {
        if let Some(node) = self.get_mut(name_or_id) {
            node.node_id = Some(node_id);
        }
    }
}

fn registry_path() -> PathBuf {
    data_file().join("nodes.toml")
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
