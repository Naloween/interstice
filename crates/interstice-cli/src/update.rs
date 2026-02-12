use interstice_core::IntersticeError;

pub fn update() -> Result<(), IntersticeError> {
    let status = self_update::backends::github::Update::configure()
        .repo_owner("Naloween")
        .repo_name("interstice")
        .bin_name("interstice")
        .show_download_progress(true)
        .current_version(env!("CARGO_PKG_VERSION"))
        .build()
        .map_err(|err| IntersticeError::Internal(format!("Update setup failed: {err}")))?
        .update()
        .map_err(|err| IntersticeError::Internal(format!("Update failed: {err}")))?;

    println!("Updated to {}", status.version());
    Ok(())
}
