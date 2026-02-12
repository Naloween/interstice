use crate::node_client::handshake_with_node;
use crate::node_registry::NodeRegistry;
use interstice_core::{
    IntersticeError, NetworkPacket,
    interstice_abi::IntersticeValue,
    packet::{read_packet, write_packet},
};

pub async fn call_query(
    node_ref: String,
    module_name: String,
    query_name: String,
    input: IntersticeValue,
) -> Result<(), IntersticeError> {
    let registry = NodeRegistry::load()?;
    let node_address = registry
        .resolve_address(&node_ref)
        .ok_or_else(|| IntersticeError::Internal("Unknown node".into()))?;
    // connect to node
    let (mut stream, _handshake) = handshake_with_node(&node_address).await?;

    // Send call query packet to node
    let request_id = uuid::Uuid::new_v4().to_string();
    let packet = NetworkPacket::QueryCall {
        module_name,
        query_name,
        input,
        request_id: request_id.clone(),
    };
    write_packet(&mut stream, &packet).await?;

    // Wait receiving query response packet from node
    let response_packet = read_packet(&mut stream).await?;
    match response_packet {
        NetworkPacket::QueryResponse {
            request_id: _response_request_id,
            result,
        } => {
            println!("Query response: {}", result);
        }
        _ => {
            println!("Unexpected packet received: {:?}", response_packet);
        }
    }

    // Close connection properly
    let packet = NetworkPacket::Close;
    write_packet(&mut stream, &packet).await?;

    Ok(())
}
