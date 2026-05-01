use crate::data_directory::nodes_dir;
use interstice_core::{IntersticeError, Node, NodeId};

pub async fn start(id: NodeId, port: u32, public_address: String) -> Result<(), IntersticeError> {
    let node = Node::load(&nodes_dir(), id, port, public_address).await?;
    node.start().await?;
    Ok(())
}
