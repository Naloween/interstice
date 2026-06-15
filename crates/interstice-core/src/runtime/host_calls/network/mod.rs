use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use interstice_abi::{
    NetworkCall, NetworkEvent, TcpCloseRequest, TcpCloseResponse, TcpConnectRequest,
    TcpConnectResponse, TcpListenRequest, TcpListenResponse, TcpSendRequest, TcpSendResponse,
    UdpBindRequest, UdpBindResponse, UdpCloseRequest, UdpCloseResponse, UdpSendToRequest,
    UdpSendToResponse,
};
use parking_lot::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::mpsc::{UnboundedSender as TokioSender, unbounded_channel};
use tokio::task::AbortHandle;
use wasmtime::{Caller, Memory};

use crate::error::IntersticeError;
use crate::runtime::Runtime;
use crate::runtime::event::EventInstance;
use crate::runtime::wasm::StoreState;

/// Out-of-band control messages to a TCP connection's driver task.
enum TcpControl {
    Send(Vec<u8>),
    Close,
}

/// Out-of-band control messages to a UDP socket's driver task.
enum UdpControl {
    SendTo { ip: String, port: u16, data: Vec<u8> },
    Close,
}

/// What a registered handle refers to. The registry maps a `u64` handle to the
/// control channel (TCP/UDP) or abort handle (listener) that drives it.
enum SocketHandle {
    Tcp(TokioSender<TcpControl>),
    Udp(TokioSender<UdpControl>),
    Listener(AbortHandle),
}

/// Runtime-side registry of live sockets for the Network authority. Sockets are
/// driven by tokio tasks; this only holds the control endpoints the synchronous
/// host calls use to talk to those tasks. Analogous to the file authority's
/// watcher registry.
pub struct NetworkState {
    next_handle: AtomicU64,
    sockets: Mutex<HashMap<u64, SocketHandle>>,
}

impl NetworkState {
    pub fn new() -> Self {
        Self {
            next_handle: AtomicU64::new(1),
            sockets: Mutex::new(HashMap::new()),
        }
    }

    fn alloc(&self) -> u64 {
        self.next_handle.fetch_add(1, Ordering::Relaxed)
    }
}

impl Default for NetworkState {
    fn default() -> Self {
        Self::new()
    }
}

type EventSender = tokio::sync::mpsc::UnboundedSender<(
    EventInstance,
    Option<crate::runtime::reducer::CompletionToken>,
)>;

fn emit(sender: &EventSender, event: NetworkEvent) {
    let _ = sender.send((EventInstance::Network(event), None));
}

/// Drive a connected TCP stream: pump incoming bytes out as `Received` events and
/// apply queued `Send`/`Close` control messages, until EOF, error, or close.
async fn run_tcp_conn(
    handle: u64,
    mut stream: TcpStream,
    mut control: tokio::sync::mpsc::UnboundedReceiver<TcpControl>,
    sender: EventSender,
) {
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        tokio::select! {
            read = stream.read(&mut buf) => match read {
                Ok(0) => {
                    emit(&sender, NetworkEvent::Closed { handle });
                    break;
                }
                Ok(n) => emit(&sender, NetworkEvent::Received { handle, data: buf[..n].to_vec() }),
                Err(err) => {
                    emit(&sender, NetworkEvent::Failed { handle, error: err.to_string() });
                    break;
                }
            },
            ctl = control.recv() => match ctl {
                Some(TcpControl::Send(data)) => {
                    if let Err(err) = stream.write_all(&data).await {
                        emit(&sender, NetworkEvent::Failed { handle, error: err.to_string() });
                        break;
                    }
                }
                Some(TcpControl::Close) | None => break,
            },
        }
    }
}

impl Runtime {
    /// Dispatch a raw network host call. Allocates/queues work and returns an
    /// acknowledgement immediately; socket results arrive asynchronously as
    /// `NetworkEvent`s on the authority holder's `on_network` reducer.
    pub fn handle_network_call(
        &self,
        call: NetworkCall,
        memory: &Memory,
        caller: &mut Caller<'_, StoreState>,
    ) -> Result<Option<i64>, IntersticeError> {
        let packed = match call {
            NetworkCall::TcpConnect(req) => {
                let response = self.net_tcp_connect(req);
                self.send_data_to_module(response, memory, caller)
            }
            NetworkCall::TcpListen(req) => {
                let response = self.net_tcp_listen(req);
                self.send_data_to_module(response, memory, caller)
            }
            NetworkCall::TcpSend(req) => {
                let response = self.net_tcp_send(req);
                self.send_data_to_module(response, memory, caller)
            }
            NetworkCall::TcpClose(req) => {
                let response = self.net_tcp_close(req);
                self.send_data_to_module(response, memory, caller)
            }
            NetworkCall::UdpBind(req) => {
                let response = self.net_udp_bind(req);
                self.send_data_to_module(response, memory, caller)
            }
            NetworkCall::UdpSendTo(req) => {
                let response = self.net_udp_send_to(req);
                self.send_data_to_module(response, memory, caller)
            }
            NetworkCall::UdpClose(req) => {
                let response = self.net_udp_close(req);
                self.send_data_to_module(response, memory, caller)
            }
        };
        Ok(Some(packed))
    }

    fn net_tcp_connect(&self, req: TcpConnectRequest) -> TcpConnectResponse {
        let addr = format!("{}:{}", req.ip, req.port);
        let handle = self.network_state.alloc();
        let (tx, rx) = unbounded_channel::<TcpControl>();
        // Register the control endpoint up front so `tcp_send` works as soon as the
        // connection is established (queued sends drain once the driver loop starts).
        self.network_state
            .sockets
            .lock()
            .insert(handle, SocketHandle::Tcp(tx));
        let sender = self.event_sender.clone();
        let net = self.network_state.clone();
        self.tokio_handle.spawn(async move {
            match TcpStream::connect(&addr).await {
                Ok(stream) => {
                    emit(&sender, NetworkEvent::Connected { handle });
                    run_tcp_conn(handle, stream, rx, sender.clone()).await;
                }
                Err(err) => {
                    emit(
                        &sender,
                        NetworkEvent::ConnectFailed {
                            handle,
                            error: err.to_string(),
                        },
                    );
                }
            }
            net.sockets.lock().remove(&handle);
        });
        TcpConnectResponse::Ok(handle)
    }

    fn net_tcp_listen(&self, req: TcpListenRequest) -> TcpListenResponse {
        let addr = format!("{}:{}", req.bind_ip, req.port);
        let listener_handle = self.network_state.alloc();
        let sender = self.event_sender.clone();
        let net = self.network_state.clone();
        let join = self.tokio_handle.spawn(async move {
            let listener = match TcpListener::bind(&addr).await {
                Ok(listener) => listener,
                Err(err) => {
                    emit(
                        &sender,
                        NetworkEvent::Failed {
                            handle: listener_handle,
                            error: err.to_string(),
                        },
                    );
                    return;
                }
            };
            loop {
                match listener.accept().await {
                    Ok((stream, peer)) => {
                        let conn_handle = net.alloc();
                        let (tx, rx) = unbounded_channel::<TcpControl>();
                        net.sockets
                            .lock()
                            .insert(conn_handle, SocketHandle::Tcp(tx));
                        emit(
                            &sender,
                            NetworkEvent::Accepted {
                                listener: listener_handle,
                                handle: conn_handle,
                                peer_ip: peer.ip().to_string(),
                                peer_port: peer.port() as u32,
                            },
                        );
                        let conn_sender = sender.clone();
                        let conn_net = net.clone();
                        tokio::spawn(async move {
                            run_tcp_conn(conn_handle, stream, rx, conn_sender).await;
                            conn_net.sockets.lock().remove(&conn_handle);
                        });
                    }
                    Err(err) => {
                        emit(
                            &sender,
                            NetworkEvent::Failed {
                                handle: listener_handle,
                                error: err.to_string(),
                            },
                        );
                        break;
                    }
                }
            }
        });
        self.network_state
            .sockets
            .lock()
            .insert(listener_handle, SocketHandle::Listener(join.abort_handle()));
        TcpListenResponse::Ok(listener_handle)
    }

    fn net_tcp_send(&self, req: TcpSendRequest) -> TcpSendResponse {
        let sockets = self.network_state.sockets.lock();
        match sockets.get(&req.handle) {
            Some(SocketHandle::Tcp(tx)) => match tx.send(TcpControl::Send(req.data)) {
                Ok(()) => TcpSendResponse::Ok,
                Err(_) => TcpSendResponse::Err("connection closed".into()),
            },
            Some(_) => TcpSendResponse::Err("handle is not a TCP connection".into()),
            None => TcpSendResponse::Err("unknown handle".into()),
        }
    }

    fn net_tcp_close(&self, req: TcpCloseRequest) -> TcpCloseResponse {
        let entry = self.network_state.sockets.lock().remove(&req.handle);
        match entry {
            Some(SocketHandle::Tcp(tx)) => {
                let _ = tx.send(TcpControl::Close);
                TcpCloseResponse::Ok
            }
            Some(SocketHandle::Listener(abort)) => {
                abort.abort();
                TcpCloseResponse::Ok
            }
            Some(SocketHandle::Udp(_)) => {
                TcpCloseResponse::Err("handle is a UDP socket; use udp_close".into())
            }
            None => TcpCloseResponse::Err("unknown handle".into()),
        }
    }

    fn net_udp_bind(&self, req: UdpBindRequest) -> UdpBindResponse {
        let addr = format!("{}:{}", req.bind_ip, req.port);
        let handle = self.network_state.alloc();
        let (tx, mut rx) = unbounded_channel::<UdpControl>();
        // Register up front so `udp_send_to` works immediately; outbound datagrams
        // queue until the bind completes inside the driver task.
        self.network_state
            .sockets
            .lock()
            .insert(handle, SocketHandle::Udp(tx));
        let sender = self.event_sender.clone();
        let net = self.network_state.clone();
        self.tokio_handle.spawn(async move {
            let socket = match UdpSocket::bind(&addr).await {
                Ok(socket) => socket,
                Err(err) => {
                    emit(
                        &sender,
                        NetworkEvent::Failed {
                            handle,
                            error: err.to_string(),
                        },
                    );
                    net.sockets.lock().remove(&handle);
                    return;
                }
            };
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                tokio::select! {
                    recv = socket.recv_from(&mut buf) => match recv {
                        Ok((n, peer)) => emit(&sender, NetworkEvent::UdpReceived {
                            handle,
                            peer_ip: peer.ip().to_string(),
                            peer_port: peer.port() as u32,
                            data: buf[..n].to_vec(),
                        }),
                        Err(err) => {
                            emit(&sender, NetworkEvent::Failed { handle, error: err.to_string() });
                            break;
                        }
                    },
                    ctl = rx.recv() => match ctl {
                        Some(UdpControl::SendTo { ip, port, data }) => {
                            if let Err(err) = socket.send_to(&data, format!("{ip}:{port}")).await {
                                emit(&sender, NetworkEvent::Failed { handle, error: err.to_string() });
                            }
                        }
                        Some(UdpControl::Close) | None => break,
                    },
                }
            }
            net.sockets.lock().remove(&handle);
        });
        UdpBindResponse::Ok(handle)
    }

    fn net_udp_send_to(&self, req: UdpSendToRequest) -> UdpSendToResponse {
        let sockets = self.network_state.sockets.lock();
        match sockets.get(&req.handle) {
            Some(SocketHandle::Udp(tx)) => match tx.send(UdpControl::SendTo {
                ip: req.ip,
                port: req.port,
                data: req.data,
            }) {
                Ok(()) => UdpSendToResponse::Ok,
                Err(_) => UdpSendToResponse::Err("socket closed".into()),
            },
            Some(_) => UdpSendToResponse::Err("handle is not a UDP socket".into()),
            None => UdpSendToResponse::Err("unknown handle".into()),
        }
    }

    fn net_udp_close(&self, req: UdpCloseRequest) -> UdpCloseResponse {
        let entry = self.network_state.sockets.lock().remove(&req.handle);
        match entry {
            Some(SocketHandle::Udp(tx)) => {
                let _ = tx.send(UdpControl::Close);
                UdpCloseResponse::Ok
            }
            Some(_) => UdpCloseResponse::Err("handle is not a UDP socket".into()),
            None => UdpCloseResponse::Err("unknown handle".into()),
        }
    }
}
