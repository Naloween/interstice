use crate::data_directory::data_file;
use interstice_core::{IntersticeError, Node, NodeId};

pub async fn start_new(port: u32) -> Result<(), IntersticeError> {
    let node = Node::new(&data_file(), port)?;
    node.start().await?;
    Ok(())
}

pub async fn start(id: NodeId, port: u32) -> Result<(), IntersticeError> {
    let node = Node::load(&data_file(), id, port).await?;
    node.start().await?;
    Ok(())
}
