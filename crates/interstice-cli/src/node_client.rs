use crate::data_directory::load_cli_identity;
use interstice_core::{
    IntersticeError, NetworkPacket,
    interstice_abi::NodeSchema,
    packet::{read_packet, write_packet},
};
use uuid::Uuid;

pub struct HandshakeInfo {
    pub node_id: String,
    pub address: String,
}

pub async fn handshake_with_node(
    address: &str,
) -> Result<(tokio::net::TcpStream, HandshakeInfo), IntersticeError> {
    let cli_identity = load_cli_identity()?;
    let mut stream = tokio::net::TcpStream::connect(address)
        .await
        .map_err(|_| IntersticeError::Internal("Failed to connect to node".into()))?;
    let packet = NetworkPacket::Handshake {
        node_id: cli_identity.cli_id,
        address: "127.0.0.1:12345".into(),
        token: cli_identity.cli_token,
    };
    write_packet(&mut stream, &packet).await?;
    let response = read_packet(&mut stream).await?;
    match response {
        NetworkPacket::Handshake {
            node_id, address, ..
        } => Ok((stream, HandshakeInfo { node_id, address })),
        _ => Err(IntersticeError::ProtocolError(
            "Expected handshake response".into(),
        )),
    }
}

pub async fn fetch_node_schema(
    address: &str,
    node_name: &str,
) -> Result<(NodeSchema, HandshakeInfo), IntersticeError> {
    let (mut stream, handshake) = handshake_with_node(address).await?;
    let request_id = Uuid::new_v4().to_string();
    let packet = NetworkPacket::SchemaRequest {
        request_id: request_id.clone(),
        node_name: node_name.to_string(),
    };
    write_packet(&mut stream, &packet).await?;
    let response = read_packet(&mut stream).await?;
    match response {
        NetworkPacket::SchemaResponse {
            request_id: response_id,
            schema,
        } => {
            if response_id != request_id {
                return Err(IntersticeError::ProtocolError(
                    "Schema response id mismatch".into(),
                ));
            }
            Ok((schema, handshake))
        }
        _ => Err(IntersticeError::ProtocolError(
            "Expected schema response".into(),
        )),
    }
}
