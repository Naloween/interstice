use crate::data_directory::load_cli_identity;
use crate::node_client::handshake_with_node;
use interstice_core::{
    IntersticeError, NetworkPacket,
    interstice_abi::IntersticeValue,
    packet::{read_packet, write_packet},
};
use tokio::net::TcpStream;
use uuid::Uuid;

pub(crate) fn default_worker_token() -> String {
    load_cli_identity()
        .map(|identity| identity.cli_token)
        .unwrap_or_else(|_| Uuid::new_v4().to_string())
}

pub(crate) async fn invoke_reducer_once(
    node_address: &str,
    module_name: &str,
    reducer_name: &str,
    input: IntersticeValue,
) -> Result<(), IntersticeError> {
    let (mut stream, _handshake) = handshake_with_node(node_address).await?;
    let packet = NetworkPacket::ReducerCall {
        module_name: module_name.to_string(),
        reducer_name: reducer_name.to_string(),
        input,
    };
    write_packet(&mut stream, &packet).await?;
    let _ = write_packet(&mut stream, &NetworkPacket::Close).await;
    Ok(())
}

pub(crate) async fn call_query_value(
    node_address: &str,
    module_name: &str,
    query_name: &str,
    input: IntersticeValue,
) -> Result<IntersticeValue, IntersticeError> {
    let request_id = Uuid::new_v4().to_string();
    let (mut stream, _handshake) = handshake_with_node(node_address).await?;

    let packet = NetworkPacket::QueryCall {
        request_id: request_id.clone(),
        module_name: module_name.to_string(),
        query_name: query_name.to_string(),
        input,
    };
    write_packet(&mut stream, &packet).await?;

    loop {
        let packet = read_packet(&mut stream).await?;
        match packet {
            NetworkPacket::QueryResponse {
                request_id: response_id,
                result,
            } if response_id == request_id => {
                let _ = write_packet(&mut stream, &NetworkPacket::Close).await;
                return Ok(result);
            }
            NetworkPacket::Error(err) => {
                return Err(IntersticeError::Internal(format!(
                    "Received error response while waiting for query: {}",
                    err
                )));
            }
            _ => {}
        }
    }
}

pub(crate) async fn handshake_with_worker_identity(
    address: &str,
    worker_peer_id: &Uuid,
    worker_token: &str,
) -> Result<TcpStream, IntersticeError> {
    let mut stream = TcpStream::connect(address)
        .await
        .map_err(|_| IntersticeError::Internal("Failed to connect to node".into()))?;

    let packet = NetworkPacket::Handshake {
        node_id: worker_peer_id.to_string(),
        address: "127.0.0.1:12345".into(),
        token: worker_token.to_string(),
    };

    write_packet(&mut stream, &packet).await?;
    let response = read_packet(&mut stream).await?;

    match response {
        NetworkPacket::Handshake { .. } => Ok(stream),
        _ => Err(IntersticeError::ProtocolError(
            "Expected handshake response".into(),
        )),
    }
}
