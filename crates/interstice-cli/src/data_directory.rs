use directories::ProjectDirs;
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
