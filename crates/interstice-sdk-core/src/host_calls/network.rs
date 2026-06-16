use crate::host_calls::{host_call, unpack};
use interstice_abi::{
    HostCall, NetworkCall, TcpCloseRequest, TcpCloseResponse, TcpConnectRequest, TcpConnectResponse,
    TcpListenRequest, TcpListenResponse, TcpSendRequest, TcpSendResponse, UdpBindRequest,
    UdpBindResponse, UdpCloseRequest, UdpCloseResponse, UdpSendToRequest, UdpSendToResponse,
};

/// Open an outbound TCP connection to `ip:port`. Returns a handle immediately;
/// the connection is established asynchronously and reported via
/// `NetworkEvent::Connected` / `ConnectFailed` to the module's `on_network` reducer.
pub fn tcp_connect(ip: String, port: u16) -> Result<u64, String> {
    tcp_connect_inner(ip, port, false, String::new())
}

/// Like [`tcp_connect`], but the host wraps the connection in TLS (a client
/// handshake validated against `server_name`) before reporting `Connected`. All
/// subsequent `tcp_send` / `Received` bytes are plaintext to the module — the
/// authority encrypts/decrypts on the wire. Use this for `https://`.
pub fn tcp_connect_tls(ip: String, port: u16, server_name: String) -> Result<u64, String> {
    tcp_connect_inner(ip, port, true, server_name)
}

fn tcp_connect_inner(
    ip: String,
    port: u16,
    tls: bool,
    server_name: String,
) -> Result<u64, String> {
    let pack = host_call(HostCall::Network(NetworkCall::TcpConnect(
        TcpConnectRequest {
            ip,
            port,
            tls,
            server_name,
        },
    )));
    match unpack::<TcpConnectResponse>(pack) {
        TcpConnectResponse::Ok(handle) => Ok(handle),
        TcpConnectResponse::Err(err) => Err(err),
    }
}

/// Bind a TCP listener on `bind_ip:port`. Returns a listener handle; accepted
/// connections arrive as `NetworkEvent::Accepted`.
pub fn tcp_listen(bind_ip: String, port: u16) -> Result<u64, String> {
    let pack = host_call(HostCall::Network(NetworkCall::TcpListen(TcpListenRequest {
        bind_ip,
        port,
    })));
    match unpack::<TcpListenResponse>(pack) {
        TcpListenResponse::Ok(handle) => Ok(handle),
        TcpListenResponse::Err(err) => Err(err),
    }
}

/// Queue `data` to be written on the TCP connection `handle`.
pub fn tcp_send(handle: u64, data: Vec<u8>) -> Result<(), String> {
    let pack = host_call(HostCall::Network(NetworkCall::TcpSend(TcpSendRequest {
        handle,
        data,
    })));
    match unpack::<TcpSendResponse>(pack) {
        TcpSendResponse::Ok => Ok(()),
        TcpSendResponse::Err(err) => Err(err),
    }
}

/// Close the TCP connection or listener `handle`.
pub fn tcp_close(handle: u64) -> Result<(), String> {
    let pack = host_call(HostCall::Network(NetworkCall::TcpClose(TcpCloseRequest {
        handle,
    })));
    match unpack::<TcpCloseResponse>(pack) {
        TcpCloseResponse::Ok => Ok(()),
        TcpCloseResponse::Err(err) => Err(err),
    }
}

/// Bind a UDP socket on `bind_ip:port`. Returns a handle; datagrams arrive as
/// `NetworkEvent::UdpReceived`.
pub fn udp_bind(bind_ip: String, port: u16) -> Result<u64, String> {
    let pack = host_call(HostCall::Network(NetworkCall::UdpBind(UdpBindRequest {
        bind_ip,
        port,
    })));
    match unpack::<UdpBindResponse>(pack) {
        UdpBindResponse::Ok(handle) => Ok(handle),
        UdpBindResponse::Err(err) => Err(err),
    }
}

/// Send a UDP datagram from socket `handle` to `ip:port`.
pub fn udp_send_to(handle: u64, ip: String, port: u16, data: Vec<u8>) -> Result<(), String> {
    let pack = host_call(HostCall::Network(NetworkCall::UdpSendTo(UdpSendToRequest {
        handle,
        ip,
        port,
        data,
    })));
    match unpack::<UdpSendToResponse>(pack) {
        UdpSendToResponse::Ok => Ok(()),
        UdpSendToResponse::Err(err) => Err(err),
    }
}

/// Close the UDP socket `handle`.
pub fn udp_close(handle: u64) -> Result<(), String> {
    let pack = host_call(HostCall::Network(NetworkCall::UdpClose(UdpCloseRequest {
        handle,
    })));
    match unpack::<UdpCloseResponse>(pack) {
        UdpCloseResponse::Ok => Ok(()),
        UdpCloseResponse::Err(err) => Err(err),
    }
}
