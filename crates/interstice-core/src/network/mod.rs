use crate::error::IntersticeError;
use crate::logger::{LogLevel, LogSource, Logger};
use crate::network::peer::PeerHandle;
use crate::network::protocol::NetworkPacket;
use crate::node::NodeId;
use crate::persistence::PeerTokenStore;
use crate::runtime::event::EventInstance;
use interstice_abi::{NodeSelection, SubscriptionEventSchema};
use packet::{read_packet, write_packet};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use uuid::Uuid;

pub mod packet;
mod peer;
pub mod protocol;

const CHANNEL_SIZE: usize = 1000;

pub struct Network {
    node_id: Uuid,
    address: String,
    peers: Arc<Mutex<HashMap<NodeId, PeerHandle>>>,
    peer_tokens: Arc<Mutex<PeerTokenStore>>,
    runtime_event_sender: mpsc::UnboundedSender<EventInstance>,
    logger: Logger,

    /// Packets coming *from* connection tasks
    packet_receiver: mpsc::Receiver<(NodeId, NetworkPacket)>,

    /// Cloned and given to connection tasks
    packet_sender: mpsc::Sender<(NodeId, NetworkPacket)>,
}

#[derive(Clone)]
pub struct NetworkHandle {
    pub node_id: Uuid,
    pub address: String,
    peers: Arc<Mutex<HashMap<NodeId, PeerHandle>>>,
    peer_tokens: Arc<Mutex<PeerTokenStore>>,
    packet_sender: mpsc::Sender<(NodeId, NetworkPacket)>,
    logger: Logger,
}

impl NetworkHandle {
    pub async fn connect_to_peer(&mut self, node_address: String) {
        if self.address == node_address {
            self.logger.log(
                "Cannot connect to self, skipping",
                LogSource::Network,
                LogLevel::Warning,
            );
            return;
        }
        let mut cloned_peers = self.peers.clone();
        let packet_sender = self.packet_sender.clone();
        let address = self.address.clone();
        let my_node_id = self.node_id.clone();
        let mut stream = TcpStream::connect(&node_address)
            .await
            .map_err(|_| IntersticeError::Internal("Failed to connect to node".into()))
            .unwrap();

        let local_token = { self.peer_tokens.lock().unwrap().local_token() };

        // Send our handshake
        write_packet(
            &mut stream,
            &NetworkPacket::Handshake {
                address: address.clone(),
                node_id: my_node_id.to_string(),
                token: local_token,
            },
        )
        .await
        .unwrap();

        if let Err(e) = handshake_incoming(
            my_node_id,
            address,
            stream,
            &mut cloned_peers,
            packet_sender,
            self.logger.clone(),
            self.peer_tokens.clone(),
            false,
        )
        .await
        {
            self.logger.log(
                &format!("Handshake failed: {:?}", e),
                LogSource::Network,
                LogLevel::Error,
            );
        }
    }

    pub async fn disconnect_peer(&self, node_id: NodeId) {
        let peer = { self.peers.lock().unwrap().remove(&node_id) };

        if let Some(peer) = peer {
            let _ = peer.send(NetworkPacket::Close).await;
            peer.close();
            self.logger.log(
                &format!("Disconnected peer {}", node_id),
                LogSource::Network,
                LogLevel::Info,
            );
        } else {
            self.logger.log(
                &format!("Attempted to disconnect unknown peer {}", node_id),
                LogSource::Network,
                LogLevel::Warning,
            );
        }
    }

    pub fn send_packet(&self, node_id: NodeId, packet: NetworkPacket) {
        let peers = self.peers.clone();
        tokio::spawn(async move {
            let peer = {
                let peers = peers.lock().unwrap();
                peers.get(&node_id).cloned()
            };
            if let Some(peer) = peer {
                let _ = peer.send(packet).await;
            }
        });
    }

    pub fn get_node_id_from_adress(&self, address: &String) -> Result<NodeId, IntersticeError> {
        for node in self.peers.lock().unwrap().values() {
            if &node.address == address {
                return Ok(node.node_id);
            }
        }
        return Err(IntersticeError::Internal(format!(
            "Couldn't find node id with address {address}. Disponible peers: \n {:?}",
            self.peers.lock().unwrap().values()
        )));
    }
    pub fn connected_peers(&self) -> Vec<(NodeId, String)> {
        self.peers
            .lock()
            .unwrap()
            .values()
            .map(|peer| (peer.node_id, peer.address.clone()))
            .collect()
    }
}

impl Network {
    pub fn new(
        node_id: Uuid,
        address: String,
        event_sender: mpsc::UnboundedSender<EventInstance>,
        peer_tokens: Arc<Mutex<PeerTokenStore>>,
        logger: Logger,
    ) -> Self {
        let (sender, receiver) = mpsc::channel(CHANNEL_SIZE);

        Self {
            node_id,
            address,
            peers: Arc::new(Mutex::new(HashMap::new())),
            peer_tokens,
            packet_receiver: receiver,
            packet_sender: sender,
            runtime_event_sender: event_sender,
            logger,
        }
    }

    pub fn get_handle(&self) -> NetworkHandle {
        NetworkHandle {
            node_id: self.node_id.clone(),
            address: self.address.clone(),
            peers: self.peers.clone(),
            peer_tokens: self.peer_tokens.clone(),
            packet_sender: self.packet_sender.clone(),
            logger: self.logger.clone(),
        }
    }

    pub fn listen(&mut self) -> Result<(), IntersticeError> {
        let peers = self.peers.clone();
        let sender = self.packet_sender.clone();
        let peer_tokens = self.peer_tokens.clone();
        let my_address = self.address.clone();
        let my_node_id = self.node_id.clone();
        let logger = self.logger.clone();
        tokio::spawn(async move {
            let listener = TcpListener::bind(&my_address)
                .await
                .map_err(|_err| IntersticeError::Internal("Failed to listen adress".into()))
                .unwrap();
            logger.log(
                &format!("Listening on {}", my_address),
                LogSource::Network,
                LogLevel::Info,
            );
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let mut cloned_peers = peers.clone();
                        let cloned_sender = sender.clone();
                        let my_address = my_address.clone();
                        let logger = logger.clone();
                        let peer_tokens = peer_tokens.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handshake_incoming(
                                my_node_id,
                                my_address,
                                stream,
                                &mut cloned_peers,
                                cloned_sender,
                                logger.clone(),
                                peer_tokens.clone(),
                                true,
                            )
                            .await
                            {
                                logger.log(
                                    &format!("Handshake failed: {:?}", e),
                                    LogSource::Network,
                                    LogLevel::Error,
                                );
                            }
                        });
                    }
                    Err(e) => logger.log(
                        &format!("Accept error: {:?}", e),
                        LogSource::Network,
                        LogLevel::Error,
                    ),
                }
            }
        });

        Ok(())
    }

    //
    // ─────────────── MAIN EVENT LOOP (CALL FROM NODE) ───────────────
    //

    pub fn run(mut self) -> JoinHandle<()> {
        return tokio::spawn(async move {
            while let Some((node_id, packet)) = self.packet_receiver.recv().await {
                match packet {
                    NetworkPacket::Handshake { .. } => {
                        self.logger.log(
                            &format!("Received unexpected handshake from {}", node_id),
                            LogSource::Network,
                            LogLevel::Warning,
                        );
                    }
                    NetworkPacket::Close => {
                        self.disconnect_peer_remote(node_id);
                    }
                    NetworkPacket::ReducerCall {
                        module_name,
                        reducer_name,
                        input,
                    } => self
                        .runtime_event_sender
                        .send(EventInstance::RemoteReducerCall {
                            requesting_node_id: node_id,
                            module_name,
                            reducer_name,
                            input,
                        })
                        .unwrap(),
                    NetworkPacket::QueryCall {
                        request_id,
                        module_name,
                        query_name,
                        input,
                    } => self
                        .runtime_event_sender
                        .send(EventInstance::RemoteQueryCall {
                            requesting_node_id: node_id,
                            request_id,
                            module_name,
                            query_name,
                            input,
                        })
                        .unwrap(),
                    NetworkPacket::QueryResponse { request_id, result } => self
                        .runtime_event_sender
                        .send(EventInstance::RemoteQueryResponse { request_id, result })
                        .unwrap(),
                    NetworkPacket::RequestSubscription(request_subscription) => self
                        .runtime_event_sender
                        .send(EventInstance::RequestSubscription {
                            requesting_node_id: node_id,
                            event: match request_subscription.event {
                                protocol::TableEvent::Insert => SubscriptionEventSchema::Insert {
                                    node_selection: NodeSelection::Current,
                                    module_name: request_subscription.module_name,
                                    table_name: request_subscription.table_name,
                                },
                                protocol::TableEvent::Update => SubscriptionEventSchema::Update {
                                    node_selection: NodeSelection::Current,
                                    module_name: request_subscription.module_name,
                                    table_name: request_subscription.table_name,
                                },
                                protocol::TableEvent::Delete => SubscriptionEventSchema::Delete {
                                    node_selection: NodeSelection::Current,
                                    module_name: request_subscription.module_name,
                                    table_name: request_subscription.table_name,
                                },
                            },
                        })
                        .unwrap(),
                    NetworkPacket::TableEvent(subscription_event) => {
                        self.runtime_event_sender
                            .send(match subscription_event {
                                protocol::TableEventInstance::TableInsertEvent {
                                    module_name,
                                    table_name,
                                    inserted_row,
                                } => EventInstance::TableInsertEvent {
                                    module_name,
                                    table_name,
                                    inserted_row,
                                },
                                protocol::TableEventInstance::TableUpdateEvent {
                                    module_name,
                                    table_name,
                                    old_row,
                                    new_row,
                                } => EventInstance::TableUpdateEvent {
                                    module_name,
                                    table_name,
                                    old_row,
                                    new_row,
                                },
                                protocol::TableEventInstance::TableDeleteEvent {
                                    module_name,
                                    table_name,
                                    deleted_row,
                                } => EventInstance::TableDeleteEvent {
                                    module_name,
                                    table_name,
                                    deleted_row,
                                },
                            })
                            .unwrap();
                    }
                    NetworkPacket::Error(err) => {
                        self.logger.log(
                            &format!("Received error from {}: {}", node_id, err),
                            LogSource::Network,
                            LogLevel::Error,
                        );
                    }
                    NetworkPacket::ModuleEvent(module_event_instance) => {
                        match module_event_instance {
                            protocol::ModuleEventInstance::Publish { wasm_binary } => self
                                .runtime_event_sender
                                .send(EventInstance::PublishModule {
                                    wasm_binary,
                                    source_node_id: node_id,
                                })
                                .unwrap(),
                            protocol::ModuleEventInstance::Remove { module_name } => self
                                .runtime_event_sender
                                .send(EventInstance::RemoveModule {
                                    module_name,
                                    source_node_id: node_id,
                                })
                                .unwrap(),
                        }
                    }
                    NetworkPacket::SchemaRequest {
                        request_id,
                        node_name,
                    } => {
                        self.runtime_event_sender
                            .send(EventInstance::SchemaRequest {
                                requesting_node_id: node_id,
                                request_id,
                                node_name,
                            })
                            .unwrap();
                    }
                    NetworkPacket::SchemaResponse { .. } => {
                        self.logger.log(
                            "Received unexpected schema response",
                            LogSource::Network,
                            LogLevel::Warning,
                        );
                    }
                }
            }
        });
    }

    fn disconnect_peer_remote(&self, node_id: NodeId) {
        let peer = { self.peers.lock().unwrap().remove(&node_id) };

        if let Some(peer) = peer {
            peer.close();
            self.logger.log(
                &format!("Peer {} disconnected", node_id),
                LogSource::Network,
                LogLevel::Info,
            );
        } else {
            self.logger.log(
                &format!("Received close from unknown peer {}", node_id),
                LogSource::Network,
                LogLevel::Info,
            );
        }
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
    close_receiver: watch::Receiver<bool>,
    logger: Logger,
) {
    let (mut reader, mut writer) = stream.into_split();
    let write_logger = logger.clone();
    let read_logger = logger;
    let mut write_close = close_receiver.clone();
    let mut read_close = close_receiver;
    let write_sender = sender.clone();
    let read_sender = sender;

    let write_loop = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = write_close.changed() => {
                    if *write_close.borrow() {
                        break;
                    }
                }
                packet = receiver.recv() => {
                    match packet {
                        Some(packet) => {
                            if let Err(e) = write_packet(&mut writer, &packet).await {
                                write_logger.log(
                                    &format!("Write error to {}: {:?}", node_id, e),
                                    LogSource::Network,
                                    LogLevel::Error,
                                );
                                let _ = write_sender
                                    .send((node_id, NetworkPacket::Close))
                                    .await;
                                break;
                            }
                        }
                        None => break,
                    }
                }
            }
        }
    });

    let read_loop = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = read_close.changed() => {
                    if *read_close.borrow() {
                        break;
                    }
                }
                packet = read_packet(&mut reader) => {
                    match packet {
                        Ok(packet) => {
                            if read_sender.send((node_id, packet)).await.is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            if is_disconnect_error(&e) {
                                read_logger.log(
                                    &format!("Connection closed by {}: {:?}", node_id, e),
                                    LogSource::Network,
                                    LogLevel::Info,
                                );
                            } else {
                                read_logger.log(
                                    &format!("Read error from {}: {:?}", node_id, e),
                                    LogSource::Network,
                                    LogLevel::Error,
                                );
                            }
                            let _ = read_sender
                                .send((node_id, NetworkPacket::Close))
                                .await;
                            break;
                        }
                    }
                }
            }
        }
    });

    let _ = tokio::join!(write_loop, read_loop);
}

fn is_disconnect_error(err: &IntersticeError) -> bool {
    match err {
        IntersticeError::Internal(message) => {
            let msg = message.to_lowercase();
            msg.contains("os error 10054")
                || msg.contains("unexpected end of file")
                || msg.contains("connection reset")
                || msg.contains("connection aborted")
                || msg.contains("broken pipe")
                || msg.contains("eof")
        }
        _ => false,
    }
}

//
// ───────────────────────── HANDSHAKE ──────────────
//
async fn handshake_incoming(
    my_node_id: NodeId,
    my_address: String,
    mut stream: TcpStream,
    peers: &mut Arc<Mutex<HashMap<NodeId, PeerHandle>>>,
    packet_sender: mpsc::Sender<(NodeId, NetworkPacket)>,
    logger: Logger,
    peer_tokens: Arc<Mutex<PeerTokenStore>>,
    send_handshake_response: bool,
) -> Result<(), IntersticeError> {
    let packet = read_packet(&mut stream).await?;

    let (peer_id_str, peer_address, peer_token) = match packet {
        NetworkPacket::Handshake {
            node_id,
            address,
            token,
        } => (node_id, address, token),
        _ => {
            return Err(IntersticeError::ProtocolError(
                "Expected handshake packet".into(),
            ));
        }
    };
    let peer_id = NodeId::parse_str(&peer_id_str).expect("Couldn't parse node id");

    {
        let mut store = peer_tokens.lock().unwrap();
        if let Some(existing) = store.get_peer_token(&peer_id) {
            if existing != peer_token {
                return Err(IntersticeError::ProtocolError(format!(
                    "Peer token mismatch for {}",
                    peer_id
                )));
            }
        } else {
            store.set_peer_token(&peer_id, peer_token)?;
        }
    }

    if send_handshake_response {
        let local_token = { peer_tokens.lock().unwrap().local_token() };
        // Reply with our handshake immediately so the remote side won't block
        // waiting for it (prevents their read from hitting EOF if we drop
        // the connection due to a duplicate).
        write_packet(
            &mut stream,
            &NetworkPacket::Handshake {
                node_id: my_node_id.to_string(),
                address: my_address,
                token: local_token,
            },
        )
        .await?;
    }

    let mut peers = peers.lock().unwrap();

    // If already connected, drop duplicate
    if peers.contains_key(&peer_id) {
        logger.log(
            &format!("Duplicate incoming connection from {}, dropping", peer_id),
            LogSource::Network,
            LogLevel::Warning,
        );
        return Ok(());
    }

    // Create channel
    let (sender, receiver) = mpsc::channel(CHANNEL_SIZE);
    let (close_sender, close_receiver) = watch::channel(false);

    // Register peer
    let handle = PeerHandle {
        node_id: peer_id,
        address: peer_address,
        sender,
        close_sender,
    };
    peers.insert(peer_id, handle);

    // Spawn connection task
    tokio::spawn(connection_task(
        peer_id,
        stream,
        receiver,
        packet_sender.clone(),
        close_receiver,
        logger.clone(),
    ));

    logger.log(
        &format!("Accepted peer {}", peer_id),
        LogSource::Network,
        LogLevel::Info,
    );

    Ok(())
}
