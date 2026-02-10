use crate::data_directory::nodes_dir;
use interstice_core::{IntersticeError, Node, NodeId};

pub async fn start(id: NodeId, port: u32) -> Result<(), IntersticeError> {
    let node = Node::load(&nodes_dir(), id, port).await?;
    node.start().await?;
    Ok(())
}
