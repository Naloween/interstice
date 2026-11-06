use directories::ProjectDirs;
use interstice_core::IntersticeError;
use std::{fs, path::PathBuf};

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

/// This CLI's identity: a key pair in `cli_identity.key`; its id is the
/// key's (DESIGN §7.4). The token file of earlier versions is unused.
pub fn load_cli_identity() -> Result<interstice_core::LocalIdentity, IntersticeError> {
    interstice_core::LocalIdentity::load_or_create(&data_file().join("cli_identity.key"), None)
}

/// Keys this CLI remembers for nodes created before keys.
pub fn cli_known_peers() -> interstice_core::KnownPeers {
    interstice_core::KnownPeers::load(Some(data_file().join("known_peers.toml")))
}
