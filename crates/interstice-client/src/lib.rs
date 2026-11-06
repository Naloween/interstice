//! A client for an Interstice node (DESIGN §10, Clients).
//!
//! It connects over mutual TLS with its own key, like a node, and then:
//! - calls reducers and waits for their commit or error ([`Client::call_reducer`]);
//! - queues many calls at once without waiting on each ([`Client::submit_reducer`]);
//! - runs queries ([`Client::call_query`]) and fetches schemas ([`Client::schema`]).
//!
//! One background task reads replies and routes them by request id; one
//! writes packets in order and flushes only when it has nothing more queued,
//! so pipelined calls share TLS records and system calls.
//!
//! ```no_run
//! # async fn demo() -> Result<(), interstice_core::IntersticeError> {
//! use interstice_client::Client;
//! use interstice_core::{LocalIdentity, KnownPeers, interstice_abi::IntersticeValue};
//! let identity = LocalIdentity::load_or_create("client.key".as_ref())?;
//! let client = Client::connect("127.0.0.1:8080", &identity, &KnownPeers::load(None)).await?;
//! client.call_reducer("hello", "hello", IntersticeValue::Vec(vec![IntersticeValue::String("world".into())])).await?;
//! # Ok(()) }
//! ```

use interstice_core::interstice_abi::{IntersticeValue, NodeSchema};
use interstice_core::packet::{read_packet, write_packet_unflushed};
use interstice_core::{
    IntersticeError, KnownPeers, LoadOutcome, LocalIdentity, MigrationPolicy, ModuleEventInstance,
    NetworkPacket,
};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, oneshot};

type ReducerReply = oneshot::Sender<Result<(), String>>;
type QueryReply = oneshot::Sender<Result<IntersticeValue, String>>;
type SchemaReply = oneshot::Sender<NodeSchema>;
type LoadReply = oneshot::Sender<LoadOutcome>;

#[derive(Default)]
struct Pending {
    reducers: HashMap<u64, ReducerReply>,
    queries: HashMap<String, QueryReply>,
    schemas: HashMap<String, SchemaReply>,
    loads: HashMap<String, LoadReply>,
}

/// A connection to one node. Cheap to share behind an `Arc`; every method
/// takes `&self`.
pub struct Client {
    node_id: uuid::Uuid,
    node_address: String,
    outgoing: mpsc::UnboundedSender<NetworkPacket>,
    pending: Arc<Mutex<Pending>>,
    next_id: AtomicU64,
    writer: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

fn closed() -> IntersticeError {
    IntersticeError::Internal("connection to the node closed".into())
}

impl Client {
    /// Connect to the node at `address`. `known` pins keys of nodes that
    /// predate key identities (DESIGN §7.4); other ids are checked against
    /// their key.
    pub async fn connect(
        address: &str,
        identity: &LocalIdentity,
        known: &KnownPeers,
    ) -> Result<Self, IntersticeError> {
        let (mut stream, spki) = identity.connect(address).await?;
        let own_address =
            std::env::var("INTERSTICE_CLI_ADDRESS").unwrap_or_else(|_| "127.0.0.1:0".to_string());
        interstice_core::packet::write_packet(
            &mut stream,
            &NetworkPacket::Handshake { node_id: identity.id, address: own_address, keys: identity.chain.clone() },
        )
        .await?;
        let (id, node_address, keys) = match read_packet(&mut stream).await? {
            NetworkPacket::Handshake { node_id, address, keys } => (node_id, address, keys),
            NetworkPacket::Refused(why) => return Err(IntersticeError::PermissionDenied(why)),
            _ => return Err(IntersticeError::ProtocolError("expected a handshake".into())),
        };
        known.bind(id, &keys, &spki)?;

        let (mut reader, mut writer) = tokio::io::split(stream);
        let pending = Arc::new(Mutex::new(Pending::default()));
        let (outgoing, mut queue) = mpsc::unbounded_channel::<NetworkPacket>();

        let writer = tokio::spawn(async move {
            while let Some(packet) = queue.recv().await {
                let mut last = matches!(packet, NetworkPacket::Close);
                if write_packet_unflushed(&mut writer, &packet).await.is_err() {
                    return;
                }
                // Drain what is already queued before one flush.
                while !last {
                    let Ok(packet) = queue.try_recv() else { break };
                    last = matches!(packet, NetworkPacket::Close);
                    if write_packet_unflushed(&mut writer, &packet).await.is_err() {
                        return;
                    }
                }
                if writer.flush().await.is_err() || last {
                    break;
                }
            }
            let _ = writer.shutdown().await;
        });

        let routes = pending.clone();
        tokio::spawn(async move {
            while let Ok(packet) = read_packet(&mut reader).await {
                let mut p = routes.lock();
                match packet {
                    NetworkPacket::ReducerResult { request_id, outcome } => {
                        if let Some(tx) = p.reducers.remove(&request_id) {
                            let _ = tx.send(outcome);
                        }
                    }
                    NetworkPacket::QueryResponse { request_id, result } => {
                        if let Some(tx) = p.queries.remove(&request_id) {
                            let _ = tx.send(result);
                        }
                    }
                    NetworkPacket::SchemaResponse { request_id, schema } => {
                        if let Some(tx) = p.schemas.remove(&request_id) {
                            let _ = tx.send(schema);
                        }
                    }
                    NetworkPacket::ModuleLoadResult { request_id, outcome } => {
                        if let Some(tx) = p.loads.remove(&request_id) {
                            let _ = tx.send(outcome);
                        }
                    }
                    NetworkPacket::Close => break,
                    _ => {}
                }
            }
            // Fail whatever is still waiting.
            let mut p = routes.lock();
            for (_, tx) in p.reducers.drain() {
                let _ = tx.send(Err("connection closed".into()));
            }
            for (_, tx) in p.queries.drain() {
                let _ = tx.send(Err("connection closed".into()));
            }
            p.schemas.clear();
            for (_, tx) in p.loads.drain() {
                let _ = tx.send(LoadOutcome::Failed("connection closed".into()));
            }
        });

        Ok(Self {
            node_id: id,
            node_address,
            outgoing,
            pending,
            next_id: AtomicU64::new(1),
            writer: Mutex::new(Some(writer)),
        })
    }

    /// The node's id (its key's).
    pub fn node_id(&self) -> uuid::Uuid {
        self.node_id
    }

    /// The address the node advertises.
    pub fn node_address(&self) -> &str {
        &self.node_address
    }

    /// Call a reducer and wait until it has committed (`Ok`) or failed.
    pub async fn call_reducer(
        &self,
        module: &str,
        reducer: &str,
        args: IntersticeValue,
    ) -> Result<(), IntersticeError> {
        self.submit_reducer(module, reducer, args)
            .await
            .map_err(IntersticeError::CallFailed)
    }

    /// Queue a reducer call now; the returned future resolves to its
    /// outcome. Calls are sent in the order they are submitted.
    pub fn submit_reducer(
        &self,
        module: &str,
        reducer: &str,
        args: IntersticeValue,
    ) -> impl Future<Output = Result<(), String>> + Send + 'static {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().reducers.insert(id, tx);
        let sent = self.outgoing.send(NetworkPacket::ReducerCall {
            request_id: Some(id),
            module_name: module.to_string(),
            reducer_name: reducer.to_string(),
            input: args,
        });
        async move {
            sent.map_err(|_| "connection closed".to_string())?;
            rx.await.unwrap_or_else(|_| Err("connection closed".into()))
        }
    }

    /// Call a reducer without asking for its outcome.
    pub fn send_reducer(&self, module: &str, reducer: &str, args: IntersticeValue) -> Result<(), IntersticeError> {
        self.outgoing
            .send(NetworkPacket::ReducerCall {
                request_id: None,
                module_name: module.to_string(),
                reducer_name: reducer.to_string(),
                input: args,
            })
            .map_err(|_| closed())
    }

    /// Run a query.
    pub async fn call_query(
        &self,
        module: &str,
        query: &str,
        args: IntersticeValue,
    ) -> Result<IntersticeValue, IntersticeError> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        self.pending.lock().queries.insert(request_id.clone(), tx);
        self.outgoing
            .send(NetworkPacket::QueryCall {
                request_id,
                module_name: module.to_string(),
                query_name: query.to_string(),
                input: args,
            })
            .map_err(|_| closed())?;
        rx.await.map_err(|_| closed())?.map_err(IntersticeError::CallFailed)
    }

    /// The node's schema as this client may see it.
    pub async fn schema(&self, node_name: &str) -> Result<NodeSchema, IntersticeError> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        self.pending.lock().schemas.insert(request_id.clone(), tx);
        self.outgoing
            .send(NetworkPacket::SchemaRequest { request_id, node_name: node_name.to_string() })
            .map_err(|_| closed())?;
        rx.await.map_err(|_| closed())
    }

    /// Load or update a module (DESIGN §3.3, §7.4). `policy` answers a
    /// migration that loses data; `grant` grants the authorities it asks for.
    pub async fn load_module(
        &self,
        wasm: Vec<u8>,
        policy: MigrationPolicy,
        grant: bool,
    ) -> Result<LoadOutcome, IntersticeError> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        self.pending.lock().loads.insert(request_id.clone(), tx);
        self.outgoing
            .send(NetworkPacket::ModuleEvent(ModuleEventInstance::Load {
                wasm_binary: wasm,
                policy,
                request_id,
                grant,
            }))
            .map_err(|_| closed())?;
        rx.await.map_err(|_| closed())
    }

    /// Unload a module, keeping its data.
    pub fn unload_module(&self, module: &str) -> Result<(), IntersticeError> {
        self.outgoing
            .send(NetworkPacket::ModuleEvent(ModuleEventInstance::Unload { module_name: module.into() }))
            .map_err(|_| closed())
    }

    /// Remove a module and delete its data.
    pub fn remove_module(&self, module: &str) -> Result<(), IntersticeError> {
        self.outgoing
            .send(NetworkPacket::ModuleEvent(ModuleEventInstance::Remove { module_name: module.into() }))
            .map_err(|_| closed())
    }

    /// Send `Close` after everything queued, and wait until it is written.
    pub async fn close(&self) {
        let _ = self.outgoing.send(NetworkPacket::Close);
        let writer = self.writer.lock().take();
        if let Some(writer) = writer {
            let _ = writer.await;
        }
    }
}

impl Drop for Client {
    /// Best effort: the writer task sends `Close` if the runtime lives on.
    fn drop(&mut self) {
        let _ = self.outgoing.send(NetworkPacket::Close);
    }
}
