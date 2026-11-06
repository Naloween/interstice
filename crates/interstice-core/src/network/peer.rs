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

/// The decision on a newly connected peer (DESIGN §7.4, admission). Its
/// connection reads nothing until it is decided, like SpacetimeDB running
/// `client_connected` before any message.
#[derive(Clone)]
pub struct Admission {
    pub peer: NodeId,
    slot: std::sync::Arc<parking_lot::Mutex<Option<tokio::sync::oneshot::Sender<bool>>>>,
}

impl std::fmt::Debug for Admission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Admission({})", self.peer)
    }
}

impl Admission {
    pub fn new(peer: NodeId) -> (Self, tokio::sync::oneshot::Receiver<bool>) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        (Self { peer, slot: std::sync::Arc::new(parking_lot::Mutex::new(Some(tx))) }, rx)
    }

    /// Admit or refuse; only the first decision counts.
    pub fn decide(&self, admit: bool) {
        if let Some(tx) = self.slot.lock().take() {
            let _ = tx.send(admit);
        }
    }
}
