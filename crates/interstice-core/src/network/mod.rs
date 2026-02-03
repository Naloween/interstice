use crate::error::IntersticeError;
use crate::network::peer::PeerHandle;
use crate::network::protocol::{NetworkPacket, RequestSubscription, SubscriptionEvent};
use crate::node::NodeId;
use packet::{read_packet, write_packet};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, mpsc};

mod packet;
mod peer;
pub mod protocol;

const CHANNEL_SIZE: usize = 1000;

pub struct Network {
    peers: Arc<Mutex<HashMap<NodeId, PeerHandle>>>,

    /// Packets coming *from* connection tasks
    receiver: mpsc::Receiver<(NodeId, NetworkPacket)>,

    /// Cloned and given to connection tasks
    sender: mpsc::Sender<(NodeId, NetworkPacket)>,
}

pub struct NetworkHandle {
    peers: Arc<Mutex<HashMap<NodeId, PeerHandle>>>,
    sender: mpsc::Sender<(NodeId, NetworkPacket)>,
}

impl NetworkHandle {
    pub async fn send_event(
        &self,
        node_id: NodeId,
        event: SubscriptionEvent,
    ) -> Result<(), IntersticeError> {
        let peers = self.peers.lock().await;
        let peer = peers.get(&node_id).ok_or(IntersticeError::UnknownPeer)?;

        peer.send(NetworkPacket::SubscriptionEvent(event)).await
    }

    pub async fn request_subscription(
        &self,
        node_id: NodeId,
        req: RequestSubscription,
    ) -> Result<(), IntersticeError> {
        let peers = self.peers.lock().await;
        let peer = peers.get(&node_id).ok_or(IntersticeError::UnknownPeer)?;

        peer.send(NetworkPacket::RequestSubscription(req)).await
    }
}

impl Network {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel(CHANNEL_SIZE);

        Self {
            peers: Arc::new(Mutex::new(HashMap::new())),
            receiver,
            sender,
        }
    }

    pub fn get_handle(&self) -> NetworkHandle {
        NetworkHandle {
            peers: self.peers.clone(),
            sender: self.sender.clone(),
        }
    }

    //
    // ─────────────── PUBLIC API USED BY YOUR NODE LAYER ───────────────
    //

    pub async fn send_event(
        &self,
        node_id: NodeId,
        event: SubscriptionEvent,
    ) -> Result<(), IntersticeError> {
        let peers = self.peers.lock().await;
        let peer = peers.get(&node_id).ok_or(IntersticeError::UnknownPeer)?;

        peer.send(NetworkPacket::SubscriptionEvent(event)).await
    }

    pub async fn request_subscription(
        &self,
        node_id: NodeId,
        req: RequestSubscription,
    ) -> Result<(), IntersticeError> {
        let peers = self.peers.lock().await;
        let peer = peers.get(&node_id).ok_or(IntersticeError::UnknownPeer)?;

        peer.send(NetworkPacket::RequestSubscription(req)).await
    }

    //
    // ─────────────── PEER CONNECTION MANAGEMENT ───────────────
    //

    pub async fn connect_to_peer(
        &mut self,
        node_address: String,
        my_node_id: NodeId,
    ) -> Result<(), IntersticeError> {
        let mut stream = TcpStream::connect(&node_address)
            .await
            .map_err(|_| IntersticeError::Internal("Failed to connect to node".into()))?;

        // Send our handshake
        write_packet(
            &mut stream,
            &NetworkPacket::Handshake {
                node_id: my_node_id.to_string(),
            },
        )
        .await?;

        let mut cloned_peers = self.peers.clone();
        let cloned_sender = self.sender.clone();
        tokio::spawn(async move {
            if let Err(e) =
                handshake_incoming(my_node_id, stream, &mut cloned_peers, cloned_sender).await
            {
                eprintln!("Handshake failed: {:?}", e);
            }
        });

        Ok(())
    }

    pub async fn listen(
        &mut self,
        bind_addr: &str,
        my_node_id: NodeId,
    ) -> Result<(), IntersticeError> {
        let listener = TcpListener::bind(bind_addr)
            .await
            .map_err(|err| IntersticeError::Internal("Failed to listen adress".into()))?;
        println!("Listening on {}", bind_addr);

        let peers = self.peers.clone();
        let sender = self.sender.clone();
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let mut cloned_peers = peers.clone();
                        let cloned_sender = sender.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handshake_incoming(
                                my_node_id,
                                stream,
                                &mut cloned_peers,
                                cloned_sender,
                            )
                            .await
                            {
                                eprintln!("Handshake failed: {:?}", e);
                            }
                        });
                    }
                    Err(e) => eprintln!("Accept error: {:?}", e),
                }
            }
        });

        Ok(())
    }

    //
    // ─────────────── MAIN EVENT LOOP (CALL FROM NODE) ───────────────
    //

    pub async fn run<F>(mut self, mut handler: F)
    where
        F: FnMut(NodeId, NetworkPacket) + Send + 'static,
    {
        tokio::spawn(async move {
            while let Some((node_id, packet)) = self.receiver.recv().await {
                handler(node_id, packet);
            }
        });
    }
}

//
// ───────────────────────── CONNECTION TASK ───────────────────────────
//

async fn connection_task(
    node_id: NodeId,
    stream: TcpStream,
    mut receiver: mpsc::Receiver<NetworkPacket>,
    sender: mpsc::Sender<(NodeId, NetworkPacket)>,
) {
    let (mut reader, mut writer) = stream.into_split();

    let write_loop = tokio::spawn(async move {
        while let Some(packet) = receiver.recv().await {
            if let Err(e) = write_packet(&mut writer, &packet).await {
                eprintln!("Write error to {}: {:?}", node_id, e);
                break;
            }
        }
    });

    let read_loop = tokio::spawn(async move {
        loop {
            match read_packet(&mut reader).await {
                Ok(packet) => {
                    if sender.send((node_id, packet)).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    eprintln!("Read error from {}: {:?}", node_id, e);
                    break;
                }
            }
        }
    });

    let _ = tokio::join!(write_loop, read_loop);
}

//
// ───────────────────────── HANDSHAKE ──────────────
//
async fn handshake_incoming(
    my_node_id: NodeId,
    mut stream: TcpStream,
    peers: &mut Arc<Mutex<HashMap<NodeId, PeerHandle>>>,
    global_sender: mpsc::Sender<(NodeId, NetworkPacket)>,
) -> Result<(), IntersticeError> {
    let packet = read_packet(&mut stream).await?;

    let peer_id_str = match packet {
        NetworkPacket::Handshake { node_id } => node_id,
        _ => {
            return Err(IntersticeError::ProtocolError(
                "Expected handshake packet".into(),
            ));
        }
    };
    let peer_id = NodeId::parse_str(&peer_id_str).expect("Couldn't parse node id");

    let mut peers = peers.lock().await;

    // If already connected, drop duplicate
    if peers.contains_key(&peer_id) {
        println!("Duplicate incoming connection from {}, dropping", peer_id);
        return Ok(());
    }

    // Reply with our handshake
    write_packet(
        &mut stream,
        &NetworkPacket::Handshake {
            node_id: my_node_id.to_string(),
        },
    )
    .await?;

    // Create channel
    let (sender, receiver) = mpsc::channel(CHANNEL_SIZE);

    // Register peer
    let handle = PeerHandle {
        node_id: peer_id,
        sender,
    };
    peers.insert(peer_id, handle);

    // Spawn connection task
    tokio::spawn(connection_task(
        peer_id,
        stream,
        receiver,
        global_sender.clone(),
    ));

    println!("Accepted peer {}", peer_id);

    Ok(())
}
