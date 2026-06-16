use serde::{Deserialize, Serialize};

use crate::interstice_abi_macros::IntersticeType;

/// Raw network authority host calls. This is the *control plane*: every call
/// allocates/queues work and returns immediately with an acknowledgement (a
/// handle or Ok/Err). It never blocks on socket I/O — connection results and
/// incoming bytes arrive asynchronously as `NetworkEvent`s delivered to the
/// authority holder's `on_network` reducer.
#[derive(Debug, Serialize, Deserialize)]
pub enum NetworkCall {
    TcpConnect(TcpConnectRequest),
    TcpListen(TcpListenRequest),
    TcpSend(TcpSendRequest),
    TcpClose(TcpCloseRequest),
    UdpBind(UdpBindRequest),
    UdpSendTo(UdpSendToRequest),
    UdpClose(UdpCloseRequest),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TcpConnectRequest {
    pub ip: String,
    pub port: u16,
    /// When true, the host performs a TLS client handshake over the connected TCP
    /// stream before delivering `Connected` (so the whole conversation is
    /// encrypted). TLS terminates host-side: wasm modules can't carry a crypto
    /// provider, so an `https://` browser still speaks plaintext to the broker and
    /// the authority does the TLS.
    pub tls: bool,
    /// SNI / certificate hostname to validate against when `tls` is set. Ignored
    /// otherwise. This is the DNS name (not the resolved IP) the request targets.
    pub server_name: String,
}

/// `Ok(handle)` = the connection attempt was registered and a handle reserved.
/// Success/failure of the actual connect arrives later as `NetworkEvent::Connected`
/// or `NetworkEvent::ConnectFailed` carrying this handle.
#[derive(Debug, Serialize, Deserialize)]
pub enum TcpConnectResponse {
    Ok(u64),
    Err(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TcpListenRequest {
    pub bind_ip: String,
    pub port: u16,
}

/// `Ok(listener_handle)` = the listener is being bound. Accepted connections
/// arrive as `NetworkEvent::Accepted` referencing this listener.
#[derive(Debug, Serialize, Deserialize)]
pub enum TcpListenResponse {
    Ok(u64),
    Err(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TcpSendRequest {
    pub handle: u64,
    pub data: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum TcpSendResponse {
    Ok,
    Err(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TcpCloseRequest {
    pub handle: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum TcpCloseResponse {
    Ok,
    Err(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UdpBindRequest {
    pub bind_ip: String,
    pub port: u16,
}

/// `Ok(handle)` = the UDP socket is bound. Datagrams arrive as
/// `NetworkEvent::UdpReceived` referencing this handle.
#[derive(Debug, Serialize, Deserialize)]
pub enum UdpBindResponse {
    Ok(u64),
    Err(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UdpSendToRequest {
    pub handle: u64,
    pub ip: String,
    pub port: u16,
    pub data: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum UdpSendToResponse {
    Ok,
    Err(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UdpCloseRequest {
    pub handle: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum UdpCloseResponse {
    Ok,
    Err(String),
}

/// Asynchronous network events. The *data plane*: delivered to the authority
/// holder's `#[reducer(on = "network")]` reducer as they happen on the socket
/// tasks. `handle` identifies the connection/socket the event belongs to.
#[derive(Debug, Deserialize, Serialize, IntersticeType, Clone)]
pub enum NetworkEvent {
    /// An outbound TCP connection was established.
    Connected { handle: u64 },
    /// An outbound TCP connection attempt failed.
    ConnectFailed { handle: u64, error: String },
    /// A listener accepted an inbound connection (`handle` is the new connection).
    /// `peer_port` is `u32` (not `u16`) because the value/event system has no `u16`.
    Accepted {
        listener: u64,
        handle: u64,
        peer_ip: String,
        peer_port: u32,
    },
    /// Bytes received on a TCP connection.
    Received { handle: u64, data: Vec<u8> },
    /// A datagram received on a UDP socket.
    UdpReceived {
        handle: u64,
        peer_ip: String,
        peer_port: u32,
        data: Vec<u8>,
    },
    /// The peer closed the connection / the stream reached EOF.
    Closed { handle: u64 },
    /// A socket-level error occurred on this handle.
    Failed { handle: u64, error: String },
}
