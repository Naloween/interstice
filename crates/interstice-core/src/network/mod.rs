use crate::error::IntersticeError;
use crate::logger::{LogLevel, LogSource, Logger};
use crate::network::peer::PeerHandle;
use crate::network::protocol::NetworkPacket;
use crate::node::NodeId;
use crate::runtime::event::EventInstance;
use crate::runtime::reducer::{Job, ReducerJob};
use crossbeam_channel::Sender as CbSender;
use interstice_abi::{NodeSelection, SubscriptionEventSchema};
use packet::{read_packet, write_packet};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use uuid::Uuid;

pub mod identity;
pub mod packet;
pub(crate) mod peer;
pub mod protocol;

const CHANNEL_SIZE: usize = 1000;

pub struct Network {
    node_id: Uuid,
    bind_address: String,
    public_address: String,
    peers: Arc<Mutex<HashMap<NodeId, PeerHandle>>>,
    trust: Arc<identity::Trust>,
    runtime_event_sender: mpsc::UnboundedSender<(
        EventInstance,
        Option<crate::runtime::reducer::CompletionToken>,
    )>,
    /// Direct bounded channel to the reducer executor — bypasses the event channel
    /// to prevent unbounded buffering of remote reducer calls.
    reducer_sender: CbSender<Job>,
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
    trust: Arc<identity::Trust>,
    packet_sender: mpsc::Sender<(NodeId, NetworkPacket)>,
    runtime_event_sender: mpsc::UnboundedSender<(
        EventInstance,
        Option<crate::runtime::reducer::CompletionToken>,
    )>,
    reducer_sender: CbSender<Job>,
    logger: Logger,
    /// Stored at construction time (from async context) so that `send_packet` can
    /// call `tokio::spawn` even when invoked from a non-tokio OS thread (e.g. wasm-reducer-*).
    tokio_handle: tokio::runtime::Handle,
}

impl NetworkHandle {
    /// Refuse a connected peer (DESIGN §7.4, admission): tell it, then drop
    /// the connection.
    pub fn disconnect_peer(&self, node_id: NodeId) {
        let peer = { self.peers.lock().remove(&node_id) };
        if let Some(peer) = peer {
            let _ = peer.sender.try_send(NetworkPacket::Close);
            peer.close();
            self.logger.log(&format!("Peer {node_id} refused and disconnected"), LogSource::Network, LogLevel::Warning);
            let _ = self.runtime_event_sender.send((EventInstance::NodeDisconnect { node_id }, None));
        }
    }

    /// Connect and handshake with the node at `node_address`. An unreachable
    /// node is an error (a module that depends on it can't load); a failed
    /// handshake is logged.
    pub async fn connect_to_peer(&mut self, node_address: String) -> Result<(), IntersticeError> {
        if self.address == node_address {
            self.logger.log(
                "Cannot connect to self, skipping",
                LogSource::Network,
                LogLevel::Warning,
            );
            return Ok(());
        }
        let mut cloned_peers = self.peers.clone();
        let packet_sender = self.packet_sender.clone();
        let address = self.address.clone();
        let my_node_id = self.node_id.clone();
        // Mutual TLS: both sides prove their keys (DESIGN §7.4).
        let (mut stream, spki) = self.trust.identity.connect(&node_address).await?;

        // Send our handshake
        write_packet(
            &mut stream,
            &NetworkPacket::Handshake {
                address: address.clone(),
                node_id: my_node_id.to_string(),
            },
        )
        .await?;

        if let Err(e) = handshake_incoming(
            my_node_id,
            address,
            stream,
            spki,
            &mut cloned_peers,
            packet_sender,
            self.runtime_event_sender.clone(),
            self.reducer_sender.clone(),
            self.logger.clone(),
            self.trust.clone(),
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
        Ok(())
    }

    /// Queue `packet` for `node_id`. Packets sent from one thread reach the
    /// peer in order: the bounded channel takes them directly, and when it is
    /// full a worker thread waits for room instead of spawning a task that
    /// could be overtaken.
    pub fn send_packet(&self, node_id: NodeId, packet: NetworkPacket) {
        let Some(peer) = self.peers.lock().get(&node_id).cloned() else {
            return;
        };
        let packet = match peer.sender.try_send(packet) {
            Ok(()) => return,
            Err(mpsc::error::TrySendError::Closed(_)) => return,
            Err(mpsc::error::TrySendError::Full(packet)) => packet,
        };
        if tokio::runtime::Handle::try_current().is_err() {
            // A plain OS thread (a reducer worker): blocking keeps the order.
            let _ = peer.sender.blocking_send(packet);
        } else {
            // On the runtime itself blocking would stall the event loop.
            self.tokio_handle.spawn(async move {
                let _ = peer.send(packet).await;
            });
        }
    }

    pub fn get_node_id_from_adress(&self, address: &String) -> Result<NodeId, IntersticeError> {
        for node in self.peers.lock().values() {
            if &node.address == address {
                return Ok(node.node_id);
            }
        }
        return Err(IntersticeError::Internal(format!(
            "Couldn't find node id with address {address}. Disponible peers: \n {:?}",
            self.peers.lock().values()
        )));
    }
}

impl Network {
    pub fn new(
        node_id: Uuid,
        bind_address: String,
        public_address: String,
        event_sender: mpsc::UnboundedSender<(
            EventInstance,
            Option<crate::runtime::reducer::CompletionToken>,
        )>,
        reducer_sender: CbSender<Job>,
        trust: Arc<identity::Trust>,
        logger: Logger,
    ) -> Self {
        let (sender, receiver) = mpsc::channel(CHANNEL_SIZE);

        Self {
            node_id,
            bind_address,
            public_address,
            peers: Arc::new(Mutex::new(HashMap::new())),
            trust,
            packet_receiver: receiver,
            packet_sender: sender,
            runtime_event_sender: event_sender,
            reducer_sender,
            logger,
        }
    }

    pub fn get_handle(&self) -> NetworkHandle {
        NetworkHandle {
            node_id: self.node_id.clone(),
            address: self.public_address.clone(),
            peers: self.peers.clone(),
            trust: self.trust.clone(),
            packet_sender: self.packet_sender.clone(),
            runtime_event_sender: self.runtime_event_sender.clone(),
            reducer_sender: self.reducer_sender.clone(),
            logger: self.logger.clone(),
            tokio_handle: tokio::runtime::Handle::current(),
        }
    }

    /// Listen on `host` instead of the host given at creation (DESIGN §7.4).
    pub fn set_bind_host(&mut self, host: &str) {
        let port = self.bind_address.rsplit(':').next().unwrap_or("0").to_string();
        self.bind_address = format!("{host}:{port}");
    }

    pub async fn listen(&mut self) -> Result<(), IntersticeError> {
        let peers = self.peers.clone();
        let sender = self.packet_sender.clone();
        let event_sender = self.runtime_event_sender.clone();
        let reducer_sender = self.reducer_sender.clone();
        let trust = self.trust.clone();
        let bind_address = self.bind_address.clone();
        let public_address = self.public_address.clone();
        let my_node_id = self.node_id.clone();
        let logger = self.logger.clone();
        let listener = TcpListener::bind(&bind_address).await.map_err(|err| {
            IntersticeError::Internal(format!(
                "Failed to listen on address {}: {}",
                bind_address, err
            ))
        })?;
        tokio::spawn(async move {
            logger.log(
                &format!("Listening on {}", bind_address),
                LogSource::Network,
                LogLevel::Info,
            );
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let mut cloned_peers = peers.clone();
                        let cloned_sender = sender.clone();
                        let event_sender = event_sender.clone();
                        let public_address = public_address.clone();
                        let logger = logger.clone();
                        let trust = trust.clone();
                        let reducer_sender = reducer_sender.clone();
                        tokio::spawn(async move {
                            let (stream, spki) = match trust.identity.accept(stream).await {
                                Ok(accepted) => accepted,
                                Err(e) => {
                                    logger.log(
                                        &format!("Refused connection: {e}"),
                                        LogSource::Network,
                                        LogLevel::Warning,
                                    );
                                    return;
                                }
                            };
                            if let Err(e) = handshake_incoming(
                                my_node_id,
                                public_address,
                                stream,
                                spki,
                                &mut cloned_peers,
                                cloned_sender,
                                event_sender.clone(),
                                reducer_sender,
                                logger.clone(),
                                trust.clone(),
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
                    // Nodes never ask each other for reducer results.
                    NetworkPacket::ReducerResult { .. } => {}
                    NetworkPacket::ReducerCall { .. } => {
                        // ReducerCall is now dispatched directly in connection_task's read_loop
                        // (via reducer_sender) — this path should not be reached.
                    }
                    NetworkPacket::QueryCall {
                        request_id,
                        module_name,
                        query_name,
                        input,
                    } => self
                        .runtime_event_sender
                        .send((
                            EventInstance::RemoteQueryCall {
                                requesting_node_id: node_id,
                                request_id,
                                module_name,
                                query_name,
                                input,
                            },
                            None,
                        ))
                        .unwrap(),
                    NetworkPacket::QueryResponse { request_id, result } => self
                        .runtime_event_sender
                        .send((
                            EventInstance::RemoteQueryResponse { request_id, result },
                            None,
                        ))
                        .unwrap(),
                    NetworkPacket::RequestSubscription(request_subscription) => self
                        .runtime_event_sender
                        .send((
                            EventInstance::RequestSubscription {
                                requesting_node_id: node_id,
                                event: match request_subscription.event {
                                    protocol::TableEvent::Insert => {
                                        SubscriptionEventSchema::Insert {
                                            node_selection: NodeSelection::Current,
                                            module_name: request_subscription.module_name,
                                            table_name: request_subscription.table_name,
                                        }
                                    }
                                    protocol::TableEvent::Update => {
                                        SubscriptionEventSchema::Update {
                                            node_selection: NodeSelection::Current,
                                            module_name: request_subscription.module_name,
                                            table_name: request_subscription.table_name,
                                        }
                                    }
                                    protocol::TableEvent::Delete => {
                                        SubscriptionEventSchema::Delete {
                                            node_selection: NodeSelection::Current,
                                            module_name: request_subscription.module_name,
                                            table_name: request_subscription.table_name,
                                        }
                                    }
                                },
                            },
                            None,
                        ))
                        .unwrap(),
                    NetworkPacket::RequestUnsubscription(request_subscription) => self
                        .runtime_event_sender
                        .send((
                            EventInstance::RequestUnsubscription {
                                requesting_node_id: node_id,
                                event: match request_subscription.event {
                                    protocol::TableEvent::Insert => {
                                        SubscriptionEventSchema::Insert {
                                            node_selection: NodeSelection::Current,
                                            module_name: request_subscription.module_name,
                                            table_name: request_subscription.table_name,
                                        }
                                    }
                                    protocol::TableEvent::Update => {
                                        SubscriptionEventSchema::Update {
                                            node_selection: NodeSelection::Current,
                                            module_name: request_subscription.module_name,
                                            table_name: request_subscription.table_name,
                                        }
                                    }
                                    protocol::TableEvent::Delete => {
                                        SubscriptionEventSchema::Delete {
                                            node_selection: NodeSelection::Current,
                                            module_name: request_subscription.module_name,
                                            table_name: request_subscription.table_name,
                                        }
                                    }
                                },
                            },
                            None,
                        ))
                        .unwrap(),
                    NetworkPacket::RequestTableSync {
                        module_name,
                        table_name,
                    } => self
                        .runtime_event_sender
                        .send((
                            EventInstance::RequestTableSync {
                                requesting_node_id: node_id,
                                module_name,
                                table_name,
                            },
                            None,
                        ))
                        .unwrap(),
                    NetworkPacket::TableSyncResponse {
                        module_name,
                        table_name,
                        rows,
                    } => self
                        .runtime_event_sender
                        .send((
                            EventInstance::RemoteTableSync {
                                source_node_id: node_id,
                                module_name,
                                table_name,
                                rows,
                            },
                            None,
                        ))
                        .unwrap(),
                    NetworkPacket::TableEvent(subscription_event) => {
                        let event = match subscription_event {
                            protocol::TableEventInstance::TableInsertEvent {
                                module_name,
                                table_name,
                                inserted_row,
                            } => EventInstance::TableInsertEvent {
                                source_node_id: Some(node_id),
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
                                source_node_id: Some(node_id),
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
                                source_node_id: Some(node_id),
                                module_name,
                                table_name,
                                deleted_row,
                            },
                        };
                        self.runtime_event_sender.send((event, None)).unwrap();
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
                            protocol::ModuleEventInstance::Load { wasm_binary, policy, request_id, grant } => self
                                .runtime_event_sender
                                .send((
                                    EventInstance::LoadModule {
                                        wasm_binary,
                                        source_node_id: node_id,
                                        policy,
                                        request_id,
                                        grant,
                                    },
                                    None,
                                ))
                                .unwrap(),
                            protocol::ModuleEventInstance::Unload { module_name } => self
                                .runtime_event_sender
                                .send((
                                    EventInstance::UnloadModule { module_name, source_node_id: node_id },
                                    None,
                                ))
                                .unwrap(),
                            protocol::ModuleEventInstance::Remove { module_name } => self
                                .runtime_event_sender
                                .send((
                                    EventInstance::RemoveModule {
                                        module_name,
                                        source_node_id: node_id,
                                    },
                                    None,
                                ))
                                .unwrap(),
                        }
                    }
                    NetworkPacket::ModuleLoadResult { outcome, .. } => {
                        self.logger.log(
                            &format!("Module load on {}: {:?}", node_id, outcome),
                            LogSource::Network,
                            LogLevel::Info,
                        );
                    }
                    NetworkPacket::SchemaRequest {
                        request_id,
                        node_name,
                    } => {
                        self.runtime_event_sender
                            .send((
                                EventInstance::SchemaRequest {
                                    requesting_node_id: node_id,
                                    request_id,
                                    node_name,
                                },
                                None,
                            ))
                            .unwrap();
                    }
                    NetworkPacket::SchemaResponse { request_id, schema } => {
                        self.runtime_event_sender
                            .send((
                                EventInstance::RemoteSchemaResponse { request_id, schema },
                                None,
                            ))
                            .unwrap();
                    }
                }
            }
        });
    }

    fn disconnect_peer_remote(&self, node_id: NodeId) {
        let peer = { self.peers.lock().remove(&node_id) };

        if let Some(peer) = peer {
            peer.close();
            self.logger.log(
                &format!("Peer {} disconnected", node_id),
                LogSource::Network,
                LogLevel::Info,
            );
            let _ = self
                .runtime_event_sender
                .send((EventInstance::NodeDisconnect { node_id }, None));
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
    stream: identity::SecureStream,
    mut receiver: mpsc::Receiver<NetworkPacket>,
    sender: mpsc::Sender<(NodeId, NetworkPacket)>,
    reducer_sender: CbSender<Job>,
    close_receiver: watch::Receiver<bool>,
    admitted: tokio::sync::oneshot::Receiver<bool>,
    logger: Logger,
) {
    let (mut reader, mut writer) = tokio::io::split(stream);
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
        match tokio::time::timeout(std::time::Duration::from_secs(30), admitted).await {
            Ok(Ok(true)) => {}
            Ok(Ok(false)) => return, // refused: the runtime disconnects it
            _ => {
                read_logger.log(
                    &format!("No admission decision for {node_id}: disconnecting"),
                    LogSource::Network,
                    LogLevel::Warning,
                );
                let _ = read_sender.send((node_id, NetworkPacket::Close)).await;
                return;
            }
        }
        loop {
            tokio::select! {
                _ = read_close.changed() => {
                    if *read_close.borrow() {
                        break;
                    }
                }
                packet = read_packet(&mut reader) => {
                    match packet {
                        Ok(NetworkPacket::ReducerCall { request_id, module_name, reducer_name, input }) => {
                            // Hot path: bypass the intermediate packet channel — dispatch
                            // directly to the crossbeam reducer queue for zero-copy dispatch.
                            // Must use blocking `send`: `try_send` dropped work when the queue
                            // filled, so clients could "succeed" millions of TCP writes while
                            // almost no reducers actually ran.
                            let job = ReducerJob {
                                module_name,
                                reducer_name,
                                input,
                                caller_node_id: node_id,
                                caller_module_name: String::new(),
                                admission: None,
                                completion: None,
                                reply: request_id,
                            };
                            let sender = reducer_sender.clone();
                            tokio::task::block_in_place(move || {
                                let _ = sender.send(job.into());
                            });
                        }
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
    mut stream: identity::SecureStream,
    spki: Vec<u8>,
    peers: &mut Arc<Mutex<HashMap<NodeId, PeerHandle>>>,
    packet_sender: mpsc::Sender<(NodeId, NetworkPacket)>,
    runtime_event_sender: mpsc::UnboundedSender<(
        EventInstance,
        Option<crate::runtime::reducer::CompletionToken>,
    )>,
    reducer_sender: CbSender<Job>,
    logger: Logger,
    trust: Arc<identity::Trust>,
    send_handshake_response: bool,
) -> Result<(), IntersticeError> {
    let packet = read_packet(&mut stream).await?;

    let (peer_id_str, peer_address) = match packet {
        NetworkPacket::Handshake { node_id, address } => (node_id, address),
        _ => {
            return Err(IntersticeError::ProtocolError(
                "Expected handshake packet".into(),
            ));
        }
    };
    // A malformed id refuses the connection; it must not panic (DESIGN §7.4).
    let peer_id = NodeId::parse_str(&peer_id_str).map_err(|_| {
        IntersticeError::ProtocolError(format!("Invalid node id in handshake: {peer_id_str:?}"))
    })?;

    // The id must be the key's (DESIGN §7.4).
    trust.known.bind(peer_id, &spki)?;

    if send_handshake_response {
        // Reply with our handshake immediately so the remote side won't block
        // waiting for it (prevents their read from hitting EOF if we drop
        // the connection due to a duplicate).
        write_packet(
            &mut stream,
            &NetworkPacket::Handshake {
                node_id: my_node_id.to_string(),
                address: my_address,
            },
        )
        .await?;
    }

    let mut peers = peers.lock();

    // If already connected, drop duplicate
    if peers.contains_key(&peer_id) {
        logger.log(
            &format!("Duplicate incoming connection from {}, dropping", peer_id),
            LogSource::Network,
            LogLevel::Warning,
        );
        return Ok(());
    }

    // Nothing is read from the peer until it is admitted (DESIGN §7.4).
    let (admission, admitted) = peer::Admission::new(peer_id);

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
        reducer_sender,
        close_receiver,
        admitted,
        logger.clone(),
    ));

    logger.log(
        &format!("Accepted peer {}", peer_id),
        LogSource::Network,
        LogLevel::Info,
    );
    let _ = runtime_event_sender.send((
        EventInstance::NodeConnect { node_id: peer_id, admission: Some(admission) },
        None,
    ));

    Ok(())
}
