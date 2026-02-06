use crate::{error::IntersticeError, network::protocol::NetworkPacket, node::NodeId};
use tokio::sync::mpsc;

#[derive(Clone, Debug)]
pub struct PeerHandle {
    pub node_id: NodeId,
    pub address: String,
    pub sender: mpsc::Sender<NetworkPacket>,
}

impl PeerHandle {
    pub async fn send(&self, packet: NetworkPacket) -> Result<(), IntersticeError> {
        self.sender
            .send(packet)
            .await
            .map_err(|_| IntersticeError::NetworkSendFailed)
    }
}
