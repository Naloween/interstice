use crate::data_directory::{cli_known_peers, load_cli_identity};
use interstice_core::{
    IntersticeError, NetworkPacket,
    interstice_abi::NodeSchema,
    packet::{read_packet, write_packet},
};
use uuid::Uuid;
use interstice_core::SecureStream;

pub struct HandshakeInfo {
    pub node_id: String,
    pub address: String,
}

pub async fn handshake_with_node(
    address: &str,
) -> Result<(SecureStream, HandshakeInfo), IntersticeError> {
    let identity = load_cli_identity()?;
    // Mutual TLS: the node proves its key, and so does this CLI (DESIGN §7.4).
    let (mut stream, spki) = identity.connect(address).await?;
    // The CLI doesn't listen for incoming connections; the address is informative.
    let cli_address =
        std::env::var("INTERSTICE_CLI_ADDRESS").unwrap_or_else(|_| "127.0.0.1:0".to_string());
    let packet = NetworkPacket::Handshake { node_id: identity.id.to_string(), address: cli_address };
    write_packet(&mut stream, &packet).await?;
    let response = read_packet(&mut stream).await?;
    let NetworkPacket::Handshake { node_id, address: node_address } = response else {
        return Err(IntersticeError::ProtocolError("Expected handshake response".into()));
    };
    let id: uuid::Uuid = node_id
        .parse()
        .map_err(|_| IntersticeError::ProtocolError(format!("Invalid node id {node_id:?}")))?;
    // The id must be the key's, and the node the registry names at this address.
    cli_known_peers().bind(id, &spki)?;
    if let Ok(registry) = crate::node_registry::NodeRegistry::load() {
        if let Some(expected) = registry
            .nodes
            .iter()
            .find(|n| n.address == address)
            .and_then(|n| n.node_id.as_deref())
        {
            if expected != node_id {
                return Err(IntersticeError::PermissionDenied(format!(
                    "the node at {address} is {node_id}, not {expected} as registered"
                )));
            }
        }
    }
    Ok((stream, HandshakeInfo { node_id, address: node_address }))
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
