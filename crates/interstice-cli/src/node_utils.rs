use crate::{data_directory::nodes_dir, node_registry::NodeRegistry};
use interstice_core::IntersticeError;

/// Remove a node from the registry and clean up its local data directory if it's a local node
pub fn remove_node_with_data(
    registry: &mut NodeRegistry,
    name_or_id: &str,
) -> Result<(), IntersticeError> {
    let removed = registry.remove(name_or_id)?;
    if removed.local {
        if let Some(node_id) = removed.node_id {
            let node_path = nodes_dir().join(node_id);
            if node_path.exists() {
                std::fs::remove_dir_all(&node_path).map_err(|err| {
                    IntersticeError::Internal(format!(
                        "Failed to remove node data at {}. \
                        Is another instance still running? Error: {err}",
                        node_path.display()
                    ))
                })?;
            }
        }
    }
    Ok(())
}
