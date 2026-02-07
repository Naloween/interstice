use crate::{error::IntersticeError, network::protocol::NetworkPacket, node::NodeId};
use tokio::sync::{mpsc, watch};

#[derive(Clone, Debug)]
pub struct PeerHandle {
    pub node_id: NodeId,
    pub address: String,
    pub sender: mpsc::Sender<NetworkPacket>,
    pub close_sender: watch::Sender<bool>,
}

impl PeerHandle {
    pub async fn send(&self, packet: NetworkPacket) -> Result<(), IntersticeError> {
        self.sender
            .send(packet)
            .await
            .map_err(|_| IntersticeError::NetworkSendFailed)
    }

    pub fn close(&self) {
        let _ = self.close_sender.send(true);
    }
}
