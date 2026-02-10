use directories::ProjectDirs;
use interstice_core::IntersticeError;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use uuid::Uuid;

pub fn data_file() -> PathBuf {
    let proj_dirs = ProjectDirs::from(
        "com",        // qualifier (reverse domain)
        "naloween",   // organization
        "interstice", // application name
    )
    .expect("Could not determine data directory");

    let dir = proj_dirs.data_dir(); // persistent app data
    fs::create_dir_all(dir).expect("Failed to create data directory");

    dir.to_path_buf()
}

pub fn nodes_dir() -> PathBuf {
    let dir = data_file().join("nodes");
    fs::create_dir_all(&dir).expect("Failed to create nodes directory");
    dir
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliIdentity {
    pub cli_id: String,
    pub cli_token: String,
}

pub fn load_cli_identity() -> Result<CliIdentity, IntersticeError> {
    let path = data_file().join("cli_identity.toml");
    if path.exists() {
        let contents = fs::read_to_string(&path).map_err(|err| {
            IntersticeError::Internal(format!(
                "Failed to read CLI identity {}: {err}",
                path.display()
            ))
        })?;
        if contents.trim().is_empty() {
            return create_and_save_identity(&path);
        }
        let identity: CliIdentity = toml::from_str(&contents).map_err(|err| {
            IntersticeError::Internal(format!(
                "Failed to parse CLI identity {}: {err}",
                path.display()
            ))
        })?;
        if identity.cli_id.trim().is_empty() || identity.cli_token.trim().is_empty() {
            return create_and_save_identity(&path);
        }
        Ok(identity)
    } else {
        create_and_save_identity(&path)
    }
}

fn create_and_save_identity(path: &PathBuf) -> Result<CliIdentity, IntersticeError> {
    let identity = CliIdentity {
        cli_id: Uuid::new_v4().to_string(),
        cli_token: Uuid::new_v4().to_string(),
    };
    let contents = toml::to_string_pretty(&identity).map_err(|err| {
        IntersticeError::Internal(format!("Failed to serialize CLI identity: {err}"))
    })?;
    fs::write(path, contents).map_err(|err| {
        IntersticeError::Internal(format!(
            "Failed to write CLI identity {}: {err}",
            path.display()
        ))
    })?;
    Ok(identity)
}
